//! A tool that runs inside the daemon: its deadline and its result, the file and media it reads or keeps, and what the
//! transcript books of it.

// Named here and not only through the glob: the reader rule's scan (`tests_registry`) places a `LedgerKind::` use by
// its file's own imports.
use theseus_protocol::LedgerKind;

use super::*;

/// An in-process call's completion detail: its duration and meta, and for a
/// call that did not succeed its words as `error`, which the kernel keeps in
/// the `action.failed` row (`theseus_kernel::failure`).
pub(super) fn failed_detail(status: ResultStatus, text: &str, dur: u64, meta: &Value) -> Value {
    let mut d = json!({"duration_ms": dur, "meta": meta});
    if status != ResultStatus::Ok && !text.trim().is_empty() {
        d["error"] = json!(text);
    }
    d
}

impl ToolRuntime {
    /// An in-process tool: its result node rides in its completion's frame.
    /// A toollet computes on a core; an async tool (DD5) waits as a task on
    /// the runtime and holds none.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
    pub(super) async fn run_inproc(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: Arc<dyn Tool>,
        call: &ToolUse,
        ran_at: Posture,
    ) -> Result<CallOutcome> {
        tc.record(&fact::tool::ToolStarted {
            session_id: tc.session_id,
            turn_id: tc.turn_id,
            tool_use_id: &call.id,
            tool: tool.name(),
            correlation_id,
            backend: tool.backend().as_str(),
        });
        let (t, input) = (tool.clone(), call.input.clone());
        let mut ctx = self.ctx_in(tc).into_owned();
        // What this session has been handed, for the stale-edit guard
        // (theseus-w2cs sub-item 2): the file tools alone, since they are
        // the ones that hand a file's bytes over and the ones that write it.
        if tool.family() == "fs" {
            ctx.reads = Some(self.reads.bind(tc.session_id));
        }
        // A toollet granted a secret reads it through the broker, bound to
        // this call (theseus-dcy). Its secrets settle first, as a turn's do,
        // since a toollet runs on a core and cannot wait.
        let bound = if self.broker.has_tool(tool.name()) {
            self.broker.settle_for_tool(tool.name()).await;
            let b = Arc::new(crate::broker::Bound {
                broker: self.broker.clone(),
                tool: tool.name().into(),
                ran_at,
                handed: Mutex::default(),
            });
            ctx.secrets = Some(b.clone());
            Some(b)
        } else {
            None
        };
        // An AWS call names its execution and itself to AWS (AWS design §3.5).
        if let Some(a) = self.aws.as_ref().filter(|_| tool.family() == "aws") {
            ctx.aws = Some(a.bind(tc.execution_id, correlation_id, &call.id));
        }
        let aws = ctx.aws.clone();
        let deadline_ms = inproc_deadline_ms(tool.as_ref());
        let deadline = Duration::from_millis(deadline_ms);
        let timed_out = || format!("timed out after {deadline_ms} ms");
        let mut aborted = false;
        let mut pre_images = Vec::new();
        // An async tool's failure may say more (an MCP server's, M7 36b):
        // that its outcome is unknown, or that its text is outside text.
        let mut failed = Value::Null;
        let (started, outcome, took) = if tool.backend() == Backend::Async {
            // A task of its own, so a panic is the call's error and not the
            // turn's, and its deadline can stop it. Only an approved call
            // reaches the private address it names.
            ctx.approved = ran_at == Posture::Approve;
            let started = theseus_protocol::now_unix_ms();
            let t0 = Instant::now();
            // A terminal's call needs its session (theseus-n88g.4).
            let run = match tool.family() {
                crate::term::FAMILY => theseus_tools::with_no_media(self.terms.run(
                    tool.name(),
                    tc.session_id,
                    &input,
                    &ctx,
                )),
                // The session's files, found on this task (theseus-c9l6).
                crate::file_read::FAMILY => self.read_file(tc, &input, &ctx),
                // This place's history, read further back (issue 44).
                _ if tool.name() == crate::view::history::HISTORY_TOOL => {
                    self.history.run(tc, &input)
                }
                // A question put to a task (issue 44, step 8).
                _ if tool.name() == crate::task_graph::ask::ASK => self.task_ask.run(tc, &input),
                // Theseus and its vault, looked at by name (issue 26).
                crate::look::FAMILY => self.look.run(tc, tool.name(), &input),
                // A profile handed a piece of work, as a task (issue 28).
                crate::profile_tool::FAMILY if tool.name() == crate::profile_tool::RUN => {
                    self.profile.run_work(tc, correlation_id, &input)
                }
                // A profile asked one question, or a conversation held to one (issue 28).
                crate::profile_tool::FAMILY => self.profile.run(
                    tc,
                    tool.name(),
                    &input,
                    correlation_id,
                    self.chat_acts
                        .offered(tc.session_id, tc.turn_id)
                        .map(|o| o.said),
                ),
                // What has been kept about the owner, asked for (theseus-6f8t0).
                crate::card::tool::FAMILY => self.card_observed.run(tc, &input),
                // By name: their family is `self`, which both share.
                _ if tool.name() == crate::rsi::backlog::tool::FILE => {
                    self.self_file.run(tc, &input)
                }
                _ if tool.name() == crate::rsi::pause_tool::PAUSE => {
                    self.self_pause.run(tc, &input)
                }
                crate::tools_defer::FAMILY => self.defer.run(tc.session_id, &input),
                // A job's handle needs its session (theseus-n8gk).
                f if Self::core(f) => self.run_job_tool(tc, tool.name(), &input, correlation_id),
                _ => t.run_async_with_media(&input, &ctx),
            };
            let mut task = tokio::spawn(run);
            let task_id = task.id();
            // A cancel or a stop aborts it (M4 18a).
            let _reachable = self.stops.track(correlation_id, task.abort_handle());
            let outcome = match tokio::time::timeout(deadline, &mut task).await {
                Ok(Ok(Ok((out, external, media)))) => Ok((out, media, external)),
                Ok(Ok(Err(f))) => {
                    failed = f.meta;
                    Err(f.message)
                }
                Ok(Err(join)) if join.is_cancelled() => {
                    aborted = true;
                    Err(join.to_string())
                }
                Ok(Err(join)) => Err(format!("the tool panicked: {join}")),
                Err(_) => {
                    task.abort();
                    Err(timed_out())
                }
            };
            // Its language-server requests are its call's spans (L2).
            if let Some(l) = &self.lsp {
                l.bind(task_id, &call.id);
            }
            (started, outcome, t0.elapsed())
        } else {
            // A free core first (theseus-a60): the deadline counts the run,
            // not the wait for one. The call's time is its run's own, timed
            // on its core: its result may wait for the turn's task, busy with
            // the frames of the calls beside it.
            // An edit tool's files are kept first (theseus-23rk).
            let rewind = crate::rewind::Rewind::of(tc, &call.id);
            let run = self
                .cpu
                .spawn(move || {
                    let held = rewind.hold(t.as_ref(), &input, &ctx);
                    let t0 = Instant::now();
                    let r = t.run_with_media(&input, &ctx);
                    (r, t0.elapsed(), rewind.settle(held, t.name()))
                })
                .await;
            let started = theseus_protocol::now_unix_ms();
            let t0 = Instant::now();
            let (outcome, took) = match tokio::time::timeout(deadline, run).await {
                Ok(Ok((Ok((out, img)), took, kept))) => {
                    pre_images = kept;
                    (Ok((out, img, None)), took)
                }
                Ok(Ok((Err(f), took, kept))) => {
                    pre_images = kept;
                    // What the tool said of its own failure (a refusal's mark) rides to the
                    // completion's detail, as an async tool's does above: dropped here, a
                    // refused edit reached the ledger as a plain failure and was filed as a fault.
                    failed = f.meta;
                    (Err(f.message), took)
                }
                Ok(Err(join)) => (Err(format!("the tool panicked: {join}")), t0.elapsed()),
                Err(_) => (Err(timed_out()), t0.elapsed()),
            };
            (started, outcome, took)
        };
        let dur = took.as_millis() as u64;
        // A task a cancel aborted (18a): the cancel settles its call, with its
        // verdict, and its result says so, riding as a late one does.
        let by_cancel = match aborted {
            true => crate::cancel::after_abort(tc.kernel, correlation_id).await,
            false => None,
        };
        let (status, mut text, mut meta, img, external) = match (outcome, &by_cancel) {
            (_, Some(a)) => {
                let (status, text, meta) = crate::cancel::aborted_result(a);
                (status, text, meta, None, None)
            }
            (Ok((o, img, external)), None) => (ResultStatus::Ok, o.text, o.meta, img, external),
            (Err(m), None) => failure(m, failed),
        };
        // What a result shows the words of (P0's rule 3): its edges ride in its frame, as a glide's did.
        let copied = crate::copies::copied(tool.name(), &mut meta);
        // A look back's answer as parts, kept in place of its words (issue 49).
        let parts = super::parts::taken(tool.name(), &mut meta);
        // An edit's diagnostics, and what arrived since for the session's
        // pending files, in a private place only (L3); none for a call a
        // cancel settled.
        let settled = by_cancel.is_none().then_some(status);
        self.lsp_onto(tc, &call.id, tool.name(), settled, &mut text, &mut meta)
            .await;
        // A transcript file.read made is spend (theseus-c9l6).
        if tool.family() == crate::file_read::FAMILY && status == ResultStatus::Ok {
            Self::book_transcript(tc, &meta);
        }
        // An image or a PDF's pages the tool read go to the blobs once; the
        // node holds the reference (theseus-9g2, theseus-c9l6).
        let image = img.and_then(|m| Self::keep_media(tc, tool.name(), m, &mut text));
        // A failed call's words ride in its completion, and so in its ledger
        // row, so the backlog can say why it failed.
        let detail = failed_detail(status, &text, dur, &meta);
        let node = self.result_node(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                duration_ms: Some(dur),
                meta: meta.clone(),
                image,
                external,
                by_ref: (!parts.is_empty()).then_some(super::parts::ByRef {
                    parts,
                    store: tc.store,
                }),
                ..ResultNode::new(&call.id, tool.name(), status, text)
            },
        );
        // A terminal's open and close (theseus-n88g.4).
        if tool.family() == crate::term::FAMILY && by_cancel.is_none() {
            fact::term::of_result(&tc.rec(), &meta);
        }
        fact::google::of_result(&tc.rec(), &meta);
        let c = Completion {
            correlation_id: correlation_id.into(),
            outcome: if status == ResultStatus::Ok {
                Outcome::Succeeded
            } else {
                Outcome::Failed
            },
            result_ref: Some(node.id.clone()),
            external_op_id: None,
            started_at_ms: started,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("{}:{}", tool.backend().as_str(), tool.name()),
            signature: None,
            cost_micros: None,
            detail: Some(detail),
        };
        for secret in bound
            .iter()
            .flat_map(|b| b.handed.lock().unwrap().split_off(0))
        {
            tc.record(&fact::tool::SecretHanded {
                tool: tool.name(),
                secret: &secret,
                correlation_id,
            });
        }
        for row in aws.iter().flat_map(|a| a.sessions()) {
            tc.record(&fact::tool::AwsSessionMinted { row: &row });
        }
        for r in aws.iter().flat_map(|a| a.requests()) {
            tc.record(&fact::tool::AwsCalled { row: &r.row });
        }
        let accepted = match by_cancel {
            Some(_) => Accepted::LateAfterCancel {
                correlation_id: correlation_id.into(),
            },
            None => {
                let mut extra = crate::rewind::records(&pre_images)?;
                extra.extend(crate::copies::edges(&node.id, &copied)?);
                self.complete(tc, &c, &node, extra)?
            }
        };
        // A call a cancel settled while it ran: its completion is recorded on
        // the action alone, and the kernel dropped the node that rode with it.
        // The call did run, and this turn still holds what it said, so the
        // transcript gets it as the call's result (theseus-0o8).
        if matches!(accepted, Accepted::LateAfterCancel { .. }) {
            let mut late = vec![super::parts::record(&node)?];
            late.extend(crate::copies::edges(&node.id, &copied)?);
            tc.store.append(&late)?;
        }
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done { status })
    }

    /// `file.read` (theseus-c9l6): the session's files, found now, and the
    /// reading in the call's own task. What its session may still spend
    /// bounds a transcript; a shared place's reading is outside text.
    fn read_file(
        &self,
        tc: &TurnCtx<'_>,
        input: &Value,
        ctx: &ToolCtx,
    ) -> theseus_tools::AsyncMediaRun {
        let nodes = match tc.store.session_nodes(tc.session_id) {
            Ok(n) => n,
            Err(e) => {
                let why = format!("this session's files could not be read: {e:#}");
                return Box::pin(std::future::ready(Err(theseus_tools::ToolFailure::new(
                    why,
                ))));
            }
        };
        let available = tc
            .kernel
            .execution(tc.execution_id)
            .ok()
            .flatten()
            .map_or(u64::MAX, |e| match tc.kernel.overdraws(&e) {
                // A limit that notifies stops no transcript (theseus-usei).
                true => u64::MAX,
                false => e.budget.available(),
            });
        let shared = tc.class != crate::places::PlaceClass::Private;
        self.files.run(
            &nodes,
            tc.store.blobs_handle(),
            tc.session_id,
            ctx,
            input,
            (shared, available),
        )
    }

    /// A transcript `file.read` made (theseus-c9l6): booked to the turn's
    /// execution as a voice call's is, with its `speech.transcribed` row.
    fn book_transcript(tc: &TurnCtx<'_>, meta: &Value) {
        let Some(sp) = meta.get("speech") else {
            return;
        };
        let (provider, model) = (
            sp["provider"].as_str().unwrap_or("deepgram"),
            sp["model"].as_str().unwrap_or(""),
        );
        let seconds = sp["seconds"].as_f64().unwrap_or(0.0).max(0.0);
        let chars = sp["chars"].as_u64().unwrap_or(0) as usize;
        let Some(price) = crate::catalog::speech_price(provider, model) else {
            return;
        };
        let cost = price.cost_micros(Duration::from_secs_f64(seconds), chars);
        let data = json!({"provider": provider, "model": model, "seconds": seconds, "chars": chars,
                          "tool": crate::file_read::READ, "file": meta["file"], "digest": meta["digest"]});
        if let Err(e) =
            tc.kernel
                .book_spend(tc.execution_id, cost, LedgerKind::SpeechTranscribed, data)
        {
            tracing::warn!(error = %e, "a transcript's spend could not be booked");
        }
    }

    /// A file a tool read, kept in the blobs for its result node: an image,
    /// or a PDF's pages with their text, whose `file.read` row is written
    /// here (theseus-c9l6). What could not be kept says so in the result.
    fn keep_media(
        tc: &TurnCtx<'_>,
        tool: &str,
        media: theseus_tools::Media,
        text: &mut String,
    ) -> Option<crate::node::Attachment> {
        let blobs = tc.store.blobs();
        let kept = match media {
            theseus_tools::Media::Image(d) => {
                crate::attach::store_image(&d.bytes, blobs).map(|(info, digest)| {
                    crate::node::Attachment {
                        name: d.name,
                        media_type: info.media_type.into(),
                        size: d.bytes.len() as u64,
                        content: crate::node::AttachmentContent::Image {
                            digest,
                            width: info.width,
                            height: info.height,
                        },
                    }
                })
            }
            theseus_tools::Media::Pdf(p) => {
                let via = if tool == "http.fetch" {
                    "http.fetch"
                } else {
                    "fs.read"
                };
                theseus_store::blocking(|| {
                    crate::attach::keep_pages(p.name, &p.bytes, &p.read, via, blobs)
                })
                .map(|(a, mut read)| {
                    (read.ms, read.capped) = (p.ms, p.capped);
                    tc.record(&fact::tool::FileRead { read: &read });
                    a
                })
            }
        };
        match kept {
            Ok(a) => Some(a),
            Err(why) => {
                text.push_str(&format!(" It is not shown: {why}."));
                None
            }
        }
    }
}
