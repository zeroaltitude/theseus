//! Credential requests under L1 (M4 18d; design `m4-boundaries` §2.4;
//! decision 15): an L1 job asks for a secret while it runs, through a
//! socket only that job can see, and the answer follows the posture the
//! job's call ran at, or the secret's own, whichever is stricter.
//!
//! - **The socket.** Before an L1 job's launch, the daemon makes
//!   `<spool>/broker/<job>/` and listens on `sock` there; the job's view
//!   binds that directory read-only at `/run/theseus/broker` (a directory,
//!   so a daemon that restarts while the job runs listens there again and
//!   the job sees the new socket). It is served until the job's call
//!   settles, a cancel and a stop included, then removed. Only that job's
//!   view holds it, and each connection is traced to the job's own wrapper
//!   (`peer`), so an L0 job of the same user, which can reach the spool's
//!   path, is refused. An L0 job gets no socket.
//! - **The helper** is this daemon's binary, bound read-only at
//!   `/run/theseus/bin/theseus-cred` (on the job's PATH), its role picked by
//!   `argv[0]`: `theseus-cred get <secret>` prints the value, for a script's
//!   `$(…)`, or an error and a nonzero exit.
//! - **The judgment.** The name must be one the broker may hand out (a grant
//!   of it, or its own `[broker.secrets.<name>]` entry), or it is an input
//!   error. The posture is the stricter of the call's `ran_at` and the
//!   secret's: open grants; notify grants with a notice; approve waits on a
//!   card, answered as any approval is (`confirm_action`, whose `judge_act`
//!   refuses a job's process), until the job's deadline.
//! - **The record.** A request is a kernel action, `cred.request`, of the
//!   job's execution, its parent the job's call (ACTION schema 4), with
//!   `secret.requested`, then `secret.granted { via: request }` or
//!   `secret.declined`. Open and notify are one frame. The value is never
//!   written: it goes from the board, over the socket, to the helper.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState, Spool};
use theseus_protocol::cred::{CredAnswer, CredAsk, CredKind, CredRequests};
use theseus_protocol::{ActionConfirmResult, ConfirmRequest, CRED_TOOL};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::fact;
use crate::outbox::Closed;
use crate::peer::Traced;
use crate::policy::Posture;
use crate::secrets::Waited;
use crate::session::SessionRecord;
use crate::Core;

/// Where the job's socket's directory is in its view.
pub const SOCKET_DIR: &str = "/run/theseus/broker";
/// The socket's name in that directory.
pub const SOCKET: &str = "sock";
/// Where the helper is in the view, and its directory, on the job's PATH.
pub const HELPER: &str = "/run/theseus/bin/theseus-cred";
pub const HELPER_DIR: &str = "/run/theseus/bin";
/// The helper's name: `argv[0]` picks its role in the daemon's binary.
pub const HELPER_NAME: &str = "theseus-cred";
/// The most requests one job's socket answers; past it, each is refused
/// unrecorded, so a job cannot fill the store with them.
pub const MAX_REQUESTS: u64 = 64;
/// The longest request line read, and how long it may take to come.
const LINE_MAX: u64 = 4096;
const READ_WAIT: Duration = Duration::from_secs(5);
/// What the job's directory keeps for a daemon that serves it again after a
/// restart: the posture its call ran at, and its command's words.
const JOB_FILE: &str = "job.json";

/// The credential requests' registry, held by the tool runtime: the socket
/// served for each running L1 job, and health's counts.
#[derive(Default)]
pub struct Creds {
    core: OnceLock<Weak<Core>>,
    served: Mutex<HashMap<String, Served>>,
    since_start: AtomicU64,
    granted: AtomicU64,
    declined: AtomicU64,
    waiting: AtomicU64,
}

/// One job's socket, while it is served.
#[derive(Debug, Clone)]
pub struct Served {
    pub dir: PathBuf,
    /// The posture the job's call ran at: approve when the operator approved
    /// it, notify otherwise (L1's, decision 1).
    pub ran_at: Posture,
    /// The job's command, in a few words (`cargo publish`), for its notices.
    pub command: String,
    requests: Arc<AtomicU64>,
}

impl Creds {
    /// The core the sockets answer for, set once it is built: each socket's
    /// task holds it by `Weak`, so none keeps the store open past a stop.
    pub fn attach(&self, core: Weak<Core>) {
        let _ = self.core.set(core);
    }

    /// Health's counts; None while there has been no request.
    pub fn health(&self) -> Option<CredRequests> {
        let c = CredRequests {
            since_start: self.since_start.load(Ordering::Relaxed),
            granted: self.granted.load(Ordering::Relaxed),
            declined: self.declined.load(Ordering::Relaxed),
            waiting: self.waiting.load(Ordering::Relaxed),
        };
        (c.since_start > 0).then_some(c)
    }

    /// The socket served for `job`, if one is.
    pub fn served(&self, job: &str) -> Option<Served> {
        self.served.lock().unwrap().get(job).cloned()
    }

    /// Serve `job`'s socket before its launch: its directory under the
    /// spool, its listener, and the task that answers it until the job
    /// settles. None, with a warning, when it cannot be made: the job runs
    /// with no socket, and its helper says so.
    pub fn serve(
        &self,
        spool: &Spool,
        job: &str,
        ran_at: Posture,
        command: &str,
    ) -> Option<PathBuf> {
        let core = self.core.get()?.clone();
        let dir = spool.dir().join("broker").join(job);
        let made = std::fs::create_dir_all(&dir)
            .and_then(|()| {
                let keep = json!({"ran_at": ran_at.as_str(), "command": command});
                std::fs::write(dir.join(JOB_FILE), keep.to_string())
            })
            .and_then(|()| listen(&dir));
        let listener = match made {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(job, error = %e, "a job's credential socket could not be made: it runs with none");
                let _ = std::fs::remove_dir_all(&dir);
                return None;
            }
        };
        let served = Served {
            dir: dir.clone(),
            ran_at,
            command: command.to_string(),
            requests: Arc::default(),
        };
        self.served.lock().unwrap().insert(job.to_string(), served);
        tokio::spawn(accept(core, job.to_string(), listener));
        Some(dir)
    }

    /// Stop serving `job` and remove its directory: its call settled, or
    /// its launch failed.
    pub fn unserve(&self, job: &str) {
        let gone = self.served.lock().unwrap().remove(job);
        if let Some(s) = gone {
            if let Err(e) = std::fs::remove_dir_all(&s.dir) {
                if e.kind() != io::ErrorKind::NotFound {
                    tracing::warn!(job, error = %e, "a job's credential socket was not removed");
                }
            }
        }
    }
}

/// A listener on `dir/sock`: a stale socket a stopped daemon left goes
/// first. A path too long for a socket's address is bound through the
/// directory's descriptor (`/proc/self/fd/<n>/sock`).
fn listen(dir: &Path) -> io::Result<tokio::net::UnixListener> {
    let path = dir.join(SOCKET);
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let l = match std::os::unix::net::UnixListener::bind(&path) {
        Err(e) if e.kind() == io::ErrorKind::InvalidInput => {
            let d = std::fs::File::open(dir)?;
            let via = PathBuf::from(format!(
                "/proc/self/fd/{}/{SOCKET}",
                std::os::fd::AsRawFd::as_raw_fd(&d)
            ));
            std::os::unix::net::UnixListener::bind(via)?
        }
        r => r?,
    };
    l.set_nonblocking(true)?;
    tokio::net::UnixListener::from_std(l)
}

/// What a job's directory kept: the posture its call ran at, and its
/// command. Approve when it cannot be read: the strictest.
fn kept(dir: &Path) -> (Posture, String) {
    let v: Value = std::fs::read(dir.join(JOB_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let ran_at = match v["ran_at"].as_str() {
        Some("open") => Posture::Open,
        Some("notify") => Posture::Notify,
        _ => Posture::Approve,
    };
    (ran_at, v["command"].as_str().unwrap_or("?").to_string())
}

/// Whether `job`'s call has settled (or is gone).
fn ended(core: &Weak<Core>, job: &str) -> bool {
    let Some(c) = core.upgrade() else { return true };
    !matches!(c.kernel.action(job), Ok(Some(a)) if !a.state.is_settled())
}

/// Answer `job`'s socket until its call settles; then the socket goes, and
/// what still waits is closed. Every frame wakes it (the push's feed), so a
/// cancel, a stop, a deadline, and a result each end it at once.
async fn accept(core: Weak<Core>, job: String, listener: tokio::net::UnixListener) {
    let Some(c) = core.upgrade() else { return };
    // The feed moves once the push is seeded (after serving, on first need).
    if let Err(e) = c.push.ensure(&c).await {
        tracing::warn!(job, error = %format!("{e:#}"), "the push could not be seeded: a job's socket closes at its next request");
    }
    let mut feed = c.push.feed();
    drop(c);
    while !ended(&core, &job) {
        tokio::select! {
            r = listener.accept() => match r {
                Ok((stream, _)) => {
                    tokio::spawn(answer(core.clone(), job.clone(), stream));
                }
                Err(e) => {
                    tracing::warn!(job, error = %e, "a job's credential socket failed");
                    break;
                }
            },
            r = feed.changed() => if r.is_err() { break },
        }
    }
    drop(listener);
    if let Some(c) = core.upgrade() {
        c.tools.creds.unserve(&job);
        c.close_cred_requests(&job, "the job ended before an answer");
    }
}

/// One connection: who is asking, its one request line, and the answer.
async fn answer(core: Weak<Core>, job: String, stream: tokio::net::UnixStream) {
    let peer = crate::peer::Peer::of_unix(&stream);
    let traced = tokio::task::spawn_blocking(move || peer.trace()).await;
    let (read, mut write) = stream.into_split();
    let mut line = String::new();
    let got = tokio::time::timeout(
        READ_WAIT,
        BufReader::new(read).take(LINE_MAX).read_line(&mut line),
    )
    .await;
    let reply = match (traced, got) {
        (Ok(Traced::Job { job: j, .. }), Ok(Ok(n))) if j == job && n > 0 => {
            match serde_json::from_str::<CredAsk>(line.trim()) {
                Ok(ask) => match core.upgrade() {
                    Some(c) => c.cred_request(&job, &ask).await,
                    None => CredAnswer::refused("the daemon is stopping"),
                },
                Err(e) => CredAnswer::refused(format!("not a request: {e}")),
            }
        }
        (Ok(Traced::Job { job: j, .. }), _) if j == job => {
            CredAnswer::refused("no request came on the socket")
        }
        (Ok(t), _) => CredAnswer::refused(format!(
            "this socket answers only job {job}'s own processes, and this one is {}",
            t.refusal().unwrap_or_else(|| "outside every job".into())
        )),
        (Err(e), _) => CredAnswer::refused(format!("the asker could not be traced: {e}")),
    };
    let mut out = serde_json::to_vec(&reply).unwrap_or_default();
    out.push(b'\n');
    let _ = write.write_all(&out).await;
    // The answer's copy here goes with `reply` and `out`.
    zeroize::Zeroize::zeroize(&mut out);
}

impl crate::toolrun::ToolRuntime {
    /// Serve an L1 job's credential socket before its launch, and bind its
    /// directory and the helper into the job's view, with the helper's
    /// directory on its PATH. False for an L0 job, which gets no socket, and
    /// for one whose socket could not be made, which runs with none.
    pub(crate) fn serve_creds(
        &self,
        spool: &Spool,
        job: &str,
        ran_at: Posture,
        argv: &[String],
        args: &mut theseus_kernel::job::WrapperArgs,
    ) -> bool {
        let Some(view) = args.sandbox.as_mut() else {
            return false;
        };
        let command = self.scrubber.scrub(&command(argv)).0;
        let Some(dir) = self.creds.serve(spool, job, ran_at, &command) else {
            return false;
        };
        view.binds.push((dir, PathBuf::from(SOCKET_DIR)));
        if let Some(exe) = self.launcher.exe() {
            view.binds.push((exe, PathBuf::from(HELPER)));
            match args.env.iter_mut().find(|(k, _)| k == "PATH") {
                Some((_, path)) => {
                    path.push(':');
                    path.push_str(HELPER_DIR);
                }
                None => args.env.push(("PATH".into(), HELPER_DIR.into())),
            }
        }
        true
    }
}

/// What a job's request is judged by: the job, and its socket's record.
struct Asking {
    job: Action,
    served: Served,
}

impl Core {
    /// Judge one request of `job`'s (decision 15), record it, and answer it:
    /// the value, or why not. An approve request waits here for its answer.
    pub(crate) async fn cred_request(self: &Arc<Self>, job: &str, ask: &CredAsk) -> CredAnswer {
        let Some(served) = self.tools.creds.served(job) else {
            return CredAnswer::refused("this job's socket is no longer served");
        };
        if served.requests.fetch_add(1, Ordering::SeqCst) >= MAX_REQUESTS {
            return CredAnswer::refused(format!(
                "this job has made {MAX_REQUESTS} requests, the most one job may"
            ));
        }
        let job = match self.kernel.action(job) {
            Ok(Some(a)) if !a.state.is_settled() => a,
            _ => return CredAnswer::refused("this job has ended"),
        };
        let asking = Asking { job, served };
        self.tools.creds.since_start.fetch_add(1, Ordering::Relaxed);
        let answer = match judge(&self.tools.broker, asking.served.ran_at, ask) {
            Judged::Refused(why) => {
                self.cred_declined_at_once(&asking, ask, &why);
                Err(why)
            }
            Judged::Grant(posture, setting) => {
                self.grant_now(&asking, ask, posture, &setting).await
            }
            Judged::Ask(setting) => self.ask_operator(&asking, ask, &setting).await,
        };
        match answer {
            Ok(v) => {
                self.tools.creds.granted.fetch_add(1, Ordering::Relaxed);
                CredAnswer {
                    value: Some(v.expose().to_string()),
                    error: None,
                }
            }
            Err(why) => {
                self.tools.creds.declined.fetch_add(1, Ordering::Relaxed);
                CredAnswer::refused(why)
            }
        }
    }

    /// Open and notify (one frame): the request, planned and granted, with
    /// its `secret.requested` and `secret.granted { via: request }`; then the
    /// value goes.
    async fn grant_now(
        &self,
        a: &Asking,
        ask: &CredAsk,
        posture: Posture,
        setting: &str,
    ) -> Result<crate::secrets::Secret, String> {
        let v = self.cred_value(&ask.name).await?;
        let how = format!("granted at {}", posture.as_str());
        let args = request_args(a, ask, posture, setting);
        let requested = fact::cred::CredRequested {
            job: &a.job,
            command: &a.served.command,
            secret: &ask.name,
            kind: ask.kind,
            posture: Some(posture),
            setting,
            outcome: "granted",
            why: None,
            correlation_id: None,
        };
        let framed = self.kernel.frame(&[&a.job.execution_id], |k| {
            let (q, records) = k.cred_request_stage(&a.job, &ask.name, args.clone(), Some(&how))?;
            k.stage(&records)?;
            let sid = Some(a.job.session_id.as_str());
            let req = fact::cred::CredRequested {
                correlation_id: Some(&q.correlation_id),
                ..requested
            };
            let granted = fact::cred::CredGranted {
                request: &q,
                job: &a.job.correlation_id,
                secret: &ask.name,
                by: &how,
            };
            k.stage(&[fact::row(&req, sid, None)?, fact::row(&granted, sid, None)?])?;
            Ok(q)
        });
        match framed {
            Ok(q) => {
                let rec = self.session_rec(&a.job.session_id);
                rec.announce(&fact::cred::CredRequested {
                    correlation_id: Some(&q.correlation_id),
                    ..requested
                });
                Ok(v)
            }
            Err(e) => {
                tracing::warn!(job = %a.job.correlation_id, error = %format!("{e:#}"), "a credential request could not be recorded: refused");
                Err(format!(
                    "the request could not be recorded ({e:#}), so it is refused"
                ))
            }
        }
    }

    /// Approve: the request waits on a card, in one frame with its
    /// `secret.requested` and the card's post; then this waits for its
    /// settle (an answer, a cancel, a stop) or the job's deadline.
    async fn ask_operator(
        self: &Arc<Self>,
        a: &Asking,
        ask: &CredAsk,
        setting: &str,
    ) -> Result<crate::secrets::Secret, String> {
        // Its value must be one to give before anyone is asked.
        self.cred_value(&ask.name).await?;
        let args = request_args(a, ask, Posture::Approve, setting);
        let target = self.outbox.target(&a.job.session_id);
        let requested = fact::cred::CredRequested {
            job: &a.job,
            command: &a.served.command,
            secret: &ask.name,
            kind: ask.kind,
            posture: Some(Posture::Approve),
            setting,
            outcome: "waiting",
            why: None,
            correlation_id: None,
        };
        let framed = self.kernel.frame(&[&a.job.execution_id], |k| {
            let (q, records) = k.cred_request_stage(&a.job, &ask.name, args.clone(), None)?;
            k.stage(&records)?;
            let req = fact::cred::CredRequested {
                correlation_id: Some(&q.correlation_id),
                ..requested
            };
            k.stage(&[fact::row(&req, Some(&a.job.session_id), None)?])?;
            let card = match &target {
                Some(t) => {
                    let body = json!({"kind": "card", "question": q.correlation_id});
                    let (post, records) =
                        self.outbox
                            .stage(&a.job.session_id, &a.job.execution_id, t, body)?;
                    k.stage(&records)?;
                    Some(post)
                }
                None => None,
            };
            Ok((q, card))
        });
        let q = match framed {
            Ok((q, card)) => {
                if let Some(post) = card {
                    self.outbox.posted(&post);
                }
                q
            }
            Err(e) => {
                return Err(format!(
                    "the request could not be recorded ({e:#}), so it is refused"
                ));
            }
        };
        let rec = self.session_rec(&a.job.session_id);
        rec.announce(&fact::cred::CredRequested {
            correlation_id: Some(&q.correlation_id),
            ..requested
        });
        if let Some(req) = self
            .store
            .get_session::<SessionRecord>(&q.session_id)
            .ok()
            .flatten()
            .and_then(|s| confirm_of(&q, &s))
        {
            rec.announce(&fact::tool::CallAsked { request: &req });
        }
        let creds = &self.tools.creds;
        creds.waiting.fetch_add(1, Ordering::Relaxed);
        let settled = self.wait_cred_settle(&q).await;
        creds.waiting.fetch_sub(1, Ordering::Relaxed);
        match settled {
            Some(s) if s.state == ActionState::Succeeded => self.cred_value(&ask.name).await,
            Some(s) => Err(match s.declined_note() {
                Some(note) => format!("declined: {note}"),
                None => format!(
                    "not granted: {}",
                    s.resolution.as_deref().unwrap_or("closed")
                ),
            }),
            None => {
                // The job's deadline came first: it lapsed.
                let why = "nobody answered before the job's deadline";
                self.close_cred_request(&q, "expiry", why);
                Err(format!("not granted: {why}"))
            }
        }
    }

    /// Wait for a request to settle, on every frame, until the job's
    /// deadline: its settled action, or None at the deadline.
    async fn wait_cred_settle(self: &Arc<Self>, q: &Action) -> Option<Action> {
        let _ = self.push.ensure(self).await;
        let mut feed = self.push.feed();
        let left = q.deadline_at_ms.saturating_sub(self.kernel.now_ms());
        let until = tokio::time::Instant::now() + Duration::from_millis(left);
        loop {
            match self.kernel.action(&q.correlation_id) {
                Ok(Some(a)) if a.state.is_settled() => return Some(a),
                Ok(Some(_)) => {}
                _ => return None,
            }
            tokio::select! {
                r = feed.changed() => if r.is_err() { return None },
                () = tokio::time::sleep_until(until) => return None,
            }
        }
    }

    /// The secret's value from the board, waited for as a turn waits (F1),
    /// or why not.
    async fn cred_value(&self, name: &str) -> Result<crate::secrets::Secret, String> {
        match self.secrets.wait(name, crate::turn::SECRET_WAIT).await {
            Waited::Ready(v) => Ok(v),
            Waited::Failed(e) => Err(format!("{name} did not resolve ({e})")),
            Waited::Resolving => Err(format!("{name} is still resolving")),
            Waited::Absent => Err(format!("{name} is not a configured secret")),
        }
    }

    /// A request declined at once (an input error, or a value the board
    /// cannot give): its `secret.requested` and `secret.declined`, one frame
    /// with no action, as invalid input writes no action.
    fn cred_declined_at_once(&self, a: &Asking, ask: &CredAsk, why: &str) {
        let requested = fact::cred::CredRequested {
            job: &a.job,
            command: &a.served.command,
            secret: &ask.name,
            kind: ask.kind,
            posture: None,
            setting: "",
            outcome: "declined",
            why: Some(why),
            correlation_id: None,
        };
        let declined = fact::cred::CredDeclined {
            request: None,
            job: &a.job.correlation_id,
            secret: &ask.name,
            by: "harness",
            why,
        };
        let sid = Some(a.job.session_id.as_str());
        let rows = fact::row(&requested, sid, None)
            .and_then(|r| Ok(vec![r, fact::row(&declined, sid, None)?]));
        match rows.and_then(|r| self.store.append(&r).map(|_| ())) {
            Ok(()) => self.session_rec(&a.job.session_id).announce(&requested),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "a declined credential request was not recorded")
            }
        }
    }

    /// Answer a waiting request (through `confirm_action`, which judged the
    /// answer): approve grants it, and the job's socket hands the value over;
    /// decline refuses it. The settle, its `secret.granted` or
    /// `secret.declined`, and the answer's row are one frame. The job's
    /// execution is not woken: no turn waits on a request.
    pub(crate) fn answer_cred_request(
        &self,
        q: &Action,
        approve: bool,
        note: Option<&str>,
        by: &str,
        via: &str,
        asker: &Traced,
    ) -> anyhow::Result<ActionConfirmResult> {
        let job = q.parent.clone().unwrap_or_default();
        let secret = q.resource.clone().unwrap_or_default();
        let answered = fact::answer::CallAnswered {
            action: q,
            approve,
            note,
            by,
            via,
            asker: asker.json(),
            trust: false,
        };
        let why = note.unwrap_or("the operator declined");
        let sid = Some(q.session_id.as_str());
        let how = format!("approved by {by}");
        let outcome = match approve {
            true => fact::row(
                &fact::cred::CredGranted {
                    request: q,
                    job: &job,
                    secret: &secret,
                    by: &how,
                },
                sid,
                None,
            )?,
            false => fact::row(
                &fact::cred::CredDeclined {
                    request: Some(q),
                    job: &job,
                    secret: &secret,
                    by,
                    why,
                },
                sid,
                None,
            )?,
        };
        let row = fact::row(&answered, sid, None)?;
        self.kernel.frame(&[&q.execution_id], |k| {
            match approve {
                true => k.grant_cred(&q.correlation_id, by)?,
                false => k.decline_action(&q.correlation_id, by, why)?,
            };
            k.stage(&[row, outcome])?;
            Ok(())
        })?;
        self.session_rec(&q.session_id).announce(&answered);
        self.card_closed(
            &q.correlation_id,
            Closed::new(if approve { "approved" } else { "declined" }, Some(by)),
        );
        self.admission.notify_waiters();
        Ok(ActionConfirmResult {
            correlation_id: q.correlation_id.clone(),
            approved: approve,
            session_id: q.session_id.clone(),
            execution_id: q.execution_id.clone(),
            resumes: false,
        })
    }

    /// Close each request of `job`'s that still waits: its job ended, or a
    /// restart left nothing to hand its answer to.
    pub(crate) fn close_cred_requests(&self, job: &str, why: &str) {
        let Ok(Some(j)) = self.kernel.action(job) else {
            return;
        };
        match self.kernel.waiting_cred_requests(&j.execution_id, job) {
            Ok(qs) => {
                for q in qs {
                    self.close_cred_request(&q, "harness", why);
                }
            }
            Err(e) => {
                tracing::warn!(job, error = %format!("{e:#}"), "a job's waiting credential requests could not be read")
            }
        }
    }

    /// Decline one waiting request, `by` the harness or the expiry, with its
    /// `secret.declined` in the frame, and settle its card.
    fn close_cred_request(&self, q: &Action, by: &str, why: &str) {
        let declined = fact::cred::CredDeclined {
            request: Some(q),
            job: q.parent.as_deref().unwrap_or_default(),
            secret: q.resource.as_deref().unwrap_or_default(),
            by,
            why,
        };
        let closed = fact::row(&declined, Some(&q.session_id), None).and_then(|row| {
            self.kernel.frame(&[&q.execution_id], |k| {
                k.decline_action(&q.correlation_id, by, why)?;
                k.stage(std::slice::from_ref(&row))?;
                Ok(())
            })
        });
        match closed {
            Ok(()) => {
                self.session_rec(&q.session_id).announce(&declined);
                self.card_closed(
                    &q.correlation_id,
                    Closed {
                        note: Some(why.to_string()),
                        ..Closed::new(if by == "expiry" { "expired" } else { "ended" }, None)
                    },
                );
            }
            // Settled meanwhile (answered, cancelled): nothing to close.
            Err(e) => {
                tracing::debug!(error = %format!("{e:#}"), "a credential request was not closed")
            }
        }
    }
}

/// What decision 15 made of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Judged {
    /// An input error: the name is not one a job may ask for, or the kind
    /// is not built yet.
    Refused(String),
    /// Granted at once, at this posture (open or notify), with what chose it.
    Grant(Posture, String),
    /// It waits for the operator, with what made it.
    Ask(String),
}

/// Decision 15 for a request of a job whose call ran at `ran_at`: the name
/// must be one the broker may hand out, and the posture is the stricter of
/// the call's and the secret's own.
pub(crate) fn judge(broker: &crate::broker::Broker, ran_at: Posture, ask: &CredAsk) -> Judged {
    if ask.kind == CredKind::Aws {
        return Judged::Refused(
            "AWS credentials through this socket are not built yet (design §2.4's AWS seam): \
             ask for a secret by its name"
                .into(),
        );
    }
    if !broker.may_hand_out(&ask.name) {
        return Judged::Refused(format!(
            "{} is not a secret a job may ask for: it has no grant and no \
             [broker.secrets.{}] entry",
            ask.name, ask.name
        ));
    }
    let (own, line) = broker.posture_of(&ask.name);
    match stricter(ran_at, own, &line) {
        (Posture::Approve, setting) => Judged::Ask(setting),
        (p, setting) => Judged::Grant(p, setting),
    }
}

/// The stricter of the posture a job's call ran at and a secret's own, with
/// what chose it in words (decision 15: "transitivity of auth settings").
pub fn stricter(ran_at: Posture, own: Posture, own_line: &str) -> (Posture, String) {
    if own > ran_at {
        (own, own_line.to_string())
    } else {
        (ran_at, format!("proc.run ran at {}", ran_at.as_str()))
    }
}

/// What a request's proposal holds: what the operator is asked about.
fn request_args(a: &Asking, ask: &CredAsk, posture: Posture, setting: &str) -> Value {
    json!({
        "job": a.job.correlation_id,
        "short": crate::task::short(&a.job.correlation_id),
        "command": a.served.command,
        "kind": ask.kind.as_str(),
        "secret": ask.name,
        "posture": posture.as_str(),
        "setting": setting,
    })
}

/// What a waiting request asks, in words: "Job a1b2c3 (`cargo publish`,
/// L1) asks for `crates_io_token`", and the setting that made it wait.
pub fn reason_of(q: &Action) -> String {
    let args = q.proposal.as_ref().map(|p| &p.args);
    let s = |k: &str| args.and_then(|a| a[k].as_str()).unwrap_or("?").to_string();
    format!(
        "Job {} (`{}`, L1) asks for `{}` ({})",
        s("short"),
        s("command"),
        s("secret"),
        s("setting")
    )
}

/// A waiting request as the confirm it is, until the job's deadline.
pub(crate) fn confirm_of(q: &Action, session: &SessionRecord) -> Option<ConfirmRequest> {
    let args = &q.proposal.as_ref()?.args;
    Some(ConfirmRequest {
        correlation_id: q.correlation_id.clone(),
        session_id: session.session_id.clone(),
        execution_id: q.execution_id.clone(),
        tool: CRED_TOOL.into(),
        input: args.clone(),
        resource: q.resource.clone(),
        reason: reason_of(q),
        by: crate::turn::OPERATOR.into(),
        requested_at_ms: q.planned_at_ms,
        expires_at_ms: q.deadline_at_ms,
        floor: false,
        budget: None,
        task: crate::task::task_ref(session),
        external_text: None,
    })
}

/// A job's command in a few words, as the narrative names a call's subject
/// (`cargo publish`): its program and a plain subcommand, nothing a secret
/// could hide in.
pub fn command(argv: &[String]) -> String {
    crate::narrative::subject("", Some(argv), None, Path::new("/"))
        .trim()
        .to_string()
}

/// After serving (a restart while L1 jobs ran): serve each running job's
/// socket again, in the directory its view still binds, and close what its
/// helper waited for, since that connection died with the old daemon. A
/// directory whose job has ended goes. Never on the start path; it reads
/// the disk, so the daemon runs it on the blocking pool.
pub fn after_serving(core: Weak<Core>) {
    let Some(c) = core.upgrade() else { return };
    let Some(spool) = c.tools.spool.clone() else {
        return;
    };
    let Ok(dir) = std::fs::read_dir(spool.dir().join("broker")) else {
        return;
    };
    for e in dir.flatten() {
        let job = e.file_name().to_string_lossy().into_owned();
        let running = matches!(c.kernel.action(&job), Ok(Some(a)) if a.state == ActionState::Dispatched)
            && spool.wrapper_lives(&job);
        c.close_cred_requests(&job, "the daemon restarted while it waited: ask again");
        if !running {
            let _ = std::fs::remove_dir_all(e.path());
            continue;
        }
        let (ran_at, command) = kept(&e.path());
        if c.tools
            .creds
            .serve(&spool, &job, ran_at, &command)
            .is_none()
        {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// The template's broker section, un-commented (design §4's item 8): an L1
/// job may ask for `github_token` alone, at notify, and for no provider's
/// key, AWS key, bot token, or web.search's key, though the wiring grants
/// that one to its toollet.
#[cfg(test)]
pub(crate) fn the_templates_broker_section(cfg: &crate::Config) {
    let broker =
        crate::broker::Broker::new(&cfg.broker, crate::secrets::SecretBoard::empty(), None);
    broker.grant_tool("web.search", &cfg.tools.web.search_key_secret);
    let askable: Vec<&str> = cfg
        .secrets
        .keys()
        .map(String::as_str)
        .filter(|n| broker.may_hand_out(n))
        .collect();
    assert_eq!(askable, ["github_token"]);
    assert_eq!(broker.posture_of("github_token").0, Posture::Notify);
}
