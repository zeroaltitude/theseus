//! A loop's provider call: the request it builds, the budget question, and what settles the call: the answer, the
//! files it accepted or hid, and an overflowed or refused one.

use super::*;

/// One attempt at the provider call, with the provider's substitution and the data floor reading
/// the call as it is made (a scope held while the future is built): the repeat the net may make
/// is made the same way (`binding::repeat_for`).
fn attempt<'a>(
    provider: &'a dyn Provider,
    sid: &'a str,
    call: crate::exits::Call,
    req: &'a crate::provider::ProviderRequest,
    on_delta: crate::provider::DeltaSink<'a>,
) -> crate::provider::ProviderFuture<'a> {
    crate::exits::for_call(call, || {
        crate::data_gate::for_session(sid, || provider.stream_message(req, on_delta))
    })
}

impl TurnRunner {
    /// The provider call, as a kernel action (§3.16): planned (with its
    /// budget reservation), dispatched, streamed, and settled. The outer
    /// error is a fault; the rest the turn reports or parks on.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub(super) async fn call_model(
        &self,
        t: &mut Turn<'_>,
        provider: &dyn Provider,
        compiled: &Compiled,
        i: u32,
    ) -> Result<Called> {
        let target = t.target;
        let proposal = Proposal {
            tool: PROVIDER_TOOL.into(),
            args: json!({"provider": target.provider, "model": target.model, "max_tokens": target.max_tokens, "loop": i, "turn_id": t.tc.turn_id, "digest": compiled.digest}),
            resource: Some(target.provider.clone()),
            policy_context: json!({"profile": target.profile}),
        };
        // Budgets are dollars (theseus-0sg), so nothing runs unpriced.
        let Some(price) = self.catalog.get(&target.model) else {
            t.record(&fact::turn::ModelUnpriced {
                model: &target.model,
            });
            return Ok(Called::Failed(Failure {
                class: "unpriced".into(),
                transient: false,
                usage_unknown: false,
                reason: format!("unpriced: {}", target.model),
                source: anyhow::anyhow!(
                    "{} has no price: add a [catalog.\"{}\"] table with its provider, \
                     context_window, max_output_tokens, and four prices",
                    target.model,
                    target.model
                ),
            }));
        };
        // The output cap at the output price, the input estimate's upper bound at the input price.
        let reserve = price.reserve_micros(target.max_tokens, compiled.estimate.upper);
        let o0 = t.trace.now_us();
        if let Err(f) = self.self_reserved(t, reserve) {
            return Ok(Called::Failed(f));
        }
        // It never needs a confirm, so its plan, authorization, and dispatch
        // are one frame, with the loop's rows in front (theseus-qa0), and
        // a recall's node and edges (M6 30b).
        // Only what the request carried (theseus-fpm2): a compile that
        // withheld the pending node leaves the turn with no recall at all,
        // so no node is written and the reply counts none.
        let rides = match t.recall.withheld {
            None => std::mem::take(&mut t.recall.rides),
            Some(why) => Self::recall_dropped(t, why),
        };
        t.recall.pending = None;
        let action = match t.tc.kernel.plan_and_dispatch(
            t.tc.guard,
            &proposal,
            RetryClass::SafeToRepeat,
            Some(self.cfg.model.timeouts.whole_call().as_millis() as u64),
            reserve,
            |_| Ok(rides),
        ) {
            Ok(a) => a,
            Err(e) => {
                if let Some(KernelError::OverBudget {
                    needed,
                    available,
                    spent,
                    limit,
                }) = e.downcast_ref::<KernelError>()
                {
                    return Ok(Called::OverBudget {
                        needed: *needed,
                        available: *available,
                        spent: *spent,
                        limit: *limit,
                    });
                }
                if let Some(KernelError::Stopped { by, .. }) = e.downcast_ref::<KernelError>() {
                    return Ok(Called::Stopped { by: by.clone() });
                }
                if let Some(KernelError::DayCeiling(r)) = e.downcast_ref::<KernelError>() {
                    return Ok(Called::Failed(self.day_ceiling_failed(t, r)));
                }
                if let Some(KernelError::HardStop(h)) = e.downcast_ref::<KernelError>() {
                    return Ok(Called::Failed(Self::hard_stopped(t, h)));
                }
                if let Some(KernelError::BurnRate(b)) = e.downcast_ref::<KernelError>() {
                    return Ok(Called::Braked(*b.clone()));
                }
                t.record(&fact::turn::ModelNotPlanned {
                    model: &target.model,
                    error: &e,
                });
                return Ok(Called::Failed(Failure {
                    class: "kernel".into(),
                    transient: false,
                    usage_unknown: false,
                    reason: format!("kernel: {e}"),
                    source: e,
                }));
            }
        };
        // The plan frame is durable: the new system blocks' syncs start now,
        // beside the request, not before it (theseus-nsp9l.4).
        Self::start_keeping(t);
        let started_ms = theseus_protocol::now_unix_ms();
        // The syncs before the model's first byte (theseus-2uby): every frame
        // the turn wrote up to its first call's dispatch, that one included.
        if i == 0 {
            let syncs = t.tc.store.turn_frames();
            t.trace.set_root(json!({ "syncs_before_call": syncs }));
        }

        // This loop answers a harness nudge, not the person (theseus-emqbm):
        // its words go to no place. Taken here, at the call the nudge rides
        // in: the loop after this one answers what came back, not the nudge.
        let quiet = std::mem::take(&mut t.nudged);
        if quiet {
            t.nudge_answers.insert(i);
        }
        let (sink, tid) = (t.tc.sink.clone(), t.tc.turn_id.to_string());
        // The characters streamed so far, thinking included (it is billed as
        // output): what a call a stop cuts is estimated from (theseus-yey).
        let streamed = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let counted = streamed.clone();
        // What the provider's own usage events have said so far: a stop that
        // cuts the stream books the call from it (theseus-k48c).
        let seen: Arc<Mutex<Option<Usage>>> = Arc::default();
        let said = seen.clone();
        let mut on_delta = move |d: Delta<'_>| match d {
            // A quiet loop's text is still counted (the cut estimate reads
            // the count, not the stream) and still written as a node.
            Delta::Text(text) => {
                counted.fetch_add(text.chars().count() as u64, Ordering::Relaxed);
                if quiet {
                    return;
                }
                To::Sink(&sink).tell(&fact::turn::ModelDelta {
                    turn_id: &tid,
                    loop_index: i,
                    text,
                })
            }
            Delta::Thinking(text) => {
                counted.fetch_add(text.chars().count() as u64, Ordering::Relaxed);
                To::Sink(&sink).tell(&fact::turn::ModelThinking {
                    turn_id: &tid,
                    loop_index: i,
                    text,
                })
            }
            Delta::ToolUseStart { .. } => {}
            Delta::Usage(u) => *said.lock().unwrap() = Some(u.clone()),
        };
        let call_started = Instant::now();
        t.record(&fact::turn::ModelCalling {
            target,
            action: &action,
            reserve,
            output_micros: price
                .reserve_parts(target.max_tokens, compiled.estimate.upper)
                .0,
            input_micros: price
                .reserve_parts(target.max_tokens, compiled.estimate.upper)
                .1,
            est_tokens: compiled.est_tokens,
            digest: &compiled.digest,
            o0,
        });
        let call_t0 = t.trace.now_us();
        // A `/stop` that lands while the call is in flight cuts its stream
        // (theseus-yey): the stop's signal wakes this, and the kernel's record
        // of it decides, so a signal that is not for this turn changes
        // nothing. A stop recorded before the call began, or in the moment
        // between its plan and here, cuts it before anything was sent.
        let armed = self.stops.arm(t.tc.execution_id);
        let outcome = {
            // The provider floor reads the session's labels as the call is made.
            let sid = t.tc.session_id;
            // The call the provider's substitution names in its row.
            let call = crate::exits::Call {
                op: None,
                session: Some(sid.to_string()),
                turn: Some(t.tc.turn_id.to_string()),
                loop_index: Some(i),
                correlation_id: Some(action.correlation_id.clone()),
            };
            // One attempt, with the provider's substitution and floor reading the call as it is
            // made: the repeat the net may make is made the same way (`binding::repeat_for`).
            let request = &compiled.request;
            let mut stream = std::pin::pin!(async {
                let first = attempt(provider, sid, call.clone(), request, &mut on_delta).await;
                let Err(e) = first else { return first };
                let Some(again) = crate::provider::binding::repeat_for(request, &e) else {
                    return Err(e);
                };
                let mut resp = attempt(provider, sid, call.clone(), &again, &mut on_delta).await?;
                resp.prefix_repeated = true;
                Ok(resp)
            });
            let mut sent = false;
            loop {
                let woken = armed.notify.notified();
                tokio::pin!(woken);
                woken.as_mut().enable();
                if let Some(by) = stopped_by(&t.tc)? {
                    break Err(Cut {
                        by,
                        sent,
                        out_chars: streamed.load(Ordering::Relaxed),
                        est_input: compiled.est_tokens,
                        prefix: cut_step::Prefix::of(t.last.as_ref(), compiled),
                        seen: seen.lock().unwrap().clone(),
                    });
                }
                if let Some(e) = self.not_sent_for_the_stop().filter(|_| !sent) {
                    break Ok(Err(e));
                }
                tokio::select! {
                    r = &mut stream => break Ok(r),
                    _ = &mut woken => {}
                }
                sent = true;
            }
        };
        drop(armed);
        match outcome {
            Err(cut) => Ok(Self::settle_cut(
                t,
                &action,
                price,
                cut,
                started_ms,
                call_started,
                i,
            )),
            Ok(Ok(mut resp)) => {
                Self::strip_standing_ack(t, &mut resp, i);
                t.record(&fact::turn::ModelAnswered {
                    resp: &resp,
                    call_t0,
                    turn_id: t.tc.turn_id,
                    loop_index: i,
                    quiet,
                });
                let node = self.settle_call(t, &action, compiled, &resp, started_ms, i)?;
                Ok(Called::Answered(Box::new((resp, node))))
            }
            Ok(Err(e)) => Ok(Called::Failed(Self::settle_failed(
                t,
                &action,
                started_ms,
                call_started,
                e,
                i,
            ))),
        }
    }

    /// The loop's call did not fit under the spend limit: ask the operator
    /// whether the spend may go back to $0 (theseus-0sg). The question goes
    /// where a tool approval goes (a `confirm.requested` to the session's
    /// clients: its Discord place, the web UI, `theseus answer`), and the
    /// turn ends parked on it; the execution waits with the reason `budget`.
    pub(super) fn ask_budget(
        &self,
        t: &mut Turn<'_>,
        session: &SessionRecord,
        needed: Micros,
        available: Micros,
        spent: Micros,
        limit: Micros,
    ) -> Result<()> {
        // A helper at its carve ends and reports; nobody answers its question.
        if helper_step::at_carve(t) {
            return Ok(());
        }
        let call = Self::budget_call(t);
        let q =
            t.tc.kernel
                .ask_budget_for(t.tc.guard, needed, call.clone())?;
        // Its card rides in the turn's next frame, the one that parks it on
        // the question (theseus-q4v).
        if let Some(target) = self.outbox.target(t.tc.session_id) {
            let (post, records) = self.outbox.stage(
                t.tc.session_id,
                t.tc.execution_id,
                &target,
                json!({"kind": "card", "question": q.correlation_id}),
            )?;
            for r in records {
                t.tc.store.defer(r)?;
            }
            t.tc.posts.lock().unwrap().push(post);
        }
        let lifetime = session.cost_usd + t.cost.unwrap_or(0.0);
        let who = match session.task {
            Some(_) => format!("Task {}", crate::task::short(t.tc.session_id)),
            None => "This session".to_string(),
        };
        let kept = Kept::of(t.tc.kernel, t.tc.execution_id);
        let question = budget_question(&who, spent, limit, needed, kept, &call);
        let req = crate::rpc::offer(ConfirmRequest {
            correlation_id: q.correlation_id.clone(),
            session_id: t.tc.session_id.into(),
            execution_id: t.tc.execution_id.into(),
            tool: BUDGET_TOOL.into(),
            input: json!({"spent_usd": micros_to_usd(spent), "limit_usd": micros_to_usd(limit), "needed_usd": micros_to_usd(needed), "available_usd": micros_to_usd(available), "model": t.target.model}),
            resource: None,
            reason: question,
            by: OPERATOR.into(),
            requested_at_ms: q.planned_at_ms,
            expires_at_ms: 0,
            floor: false,
            budget: Some(BudgetAsk {
                spent_usd: micros_to_usd(spent),
                limit_usd: micros_to_usd(limit),
                needed_usd: micros_to_usd(needed),
                lifetime_usd: lifetime,
            }),
            task: crate::task::task_ref(session),
            change: None,
            ..Default::default()
        });
        t.record(&fact::turn::BudgetAsked {
            request: &req,
            session_id: t.tc.session_id,
            model: &t.target.model,
            limit,
            spent,
            needed,
            available,
        });
        t.budget_question = Some(q.correlation_id);
        t.stop_reason = "budget".into();
        Ok(())
    }

    /// The operator approved a reset for a call that alone needed more than
    /// the whole limit, and the retry's call still does (theseus-kks): no
    /// reset can make it fit, so the turn fails instead of asking the same
    /// question again. Its failed notice names the figures and the remedies,
    /// and the execution waits on input (a task ends and reports it).
    pub(super) fn over_limit(t: &mut Turn<'_>, needed: Micros, limit: Micros) -> Failure {
        let kept = Kept::of(t.tc.kernel, t.tc.execution_id);
        let lower = lower_cap(&t.target.profile, u64::from(t.target.max_tokens));
        let msg = if needed > limit {
            format!(
                "the call to {} alone reserves {}, more than the whole {} spend limit, so a \
                 reset cannot make it fit and the turn ends here. Raise `[kernel] \
                 spend_limit_usd` above {}, or {lower}, then send a message to try again",
                t.target.model,
                narrative::dollars(needed),
                narrative::dollars(limit),
                narrative::dollars(needed),
            )
        } else if kept.total() > 0 {
            // What a reset leaves held (theseus-6g6): the call fits the
            // limit, and not what the held amounts leave of it.
            format!(
                "the call to {} reserves {}, and a reset cannot make it fit: of the whole {} \
                 spend limit, {}, and a reset leaves it held, so the turn ends here. Raise \
                 `[kernel] spend_limit_usd` above {}, or {lower}, then send a message to try \
                 again",
                t.target.model,
                narrative::dollars(needed),
                narrative::dollars(limit),
                kept.clause(),
                narrative::dollars(needed + kept.total()),
            )
        } else {
            format!(
                "the call to {} reserves {}, and it still does not fit under the {} spend limit \
                 after the reset, so the turn ends here. Raise `[kernel] spend_limit_usd`, or \
                 {lower}, then send a message to try again",
                t.target.model,
                narrative::dollars(needed),
                narrative::dollars(limit),
            )
        };
        t.record(&fact::turn::OverLimit {
            message: &msg,
            target: t.target,
            needed,
            limit,
            held: kept.total(),
        });
        Failure {
            class: "over_limit".into(),
            transient: false,
            usage_unknown: false,
            reason: "over_limit".into(),
            source: anyhow::anyhow!("{msg}"),
        }
    }

    /// Settle a call that answered: the assistant node rides in the frame of
    /// the call's completion, and the call's usage, cost, and text join the
    /// turn's books.
    fn settle_call(
        &self,
        t: &mut Turn<'_>,
        action: &Action,
        compiled: &Compiled,
        resp: &ModelResponse,
        started_ms: u64,
        i: u32,
    ) -> Result<Node> {
        let target = t.target;
        // A call is priced at the rate in force when it was made: a provider's
        // off-peak hours are cheaper (DeepSeek's, `catalog::price_period_at`).
        let call_cost = self
            .catalog
            .cost_usd_at(&resp.model, &resp.usage, started_ms)
            .or_else(|| {
                self.catalog
                    .cost_usd_at(&target.model, &resp.usage, started_ms)
            });
        t.cost = match (t.cost, call_cost) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
        let node = Node::assistant(
            t.tc.session_id,
            t.tc.turn_id,
            i,
            Body::AssistantMessage {
                blocks: resp.content.clone(),
                model: resp.model.clone(),
                provider: target.provider.clone(),
                stop_reason: resp.stop_reason.clone(),
                usage: resp.usage.clone(),
                cost_usd: call_cost,
                catalog_version: Some(self.catalog.version.clone()),
                request_id: resp.request_id.clone(),
                correlation_id: Some(action.correlation_id.clone()),
                compilation_id: Some(compiled.compilation.id.clone()),
                request_digest: Some(compiled.digest.clone()),
                effort: crate::compiler::effort::placed(&compiled.request),
            },
        );
        // The budget settles at the real cost, each token class at its own
        // price; a served model the catalog lacks is priced as the target.
        let cost = self
            .catalog
            .get(&resp.model)
            .or_else(|| self.catalog.get(&target.model))
            .map_or(action.reserved_micros, |e| {
                e.cost_micros_at(&resp.usage, started_ms)
            });
        // The call's usage joins the turn's books with its cost, before its
        // frame is written: a turn that faults on that frame still books
        // what the provider charged (R1).
        add_usage(&mut t.usage, &resp.usage);
        let call = fact::turn::ProviderCall {
            loop_index: i,
            provider: &target.provider,
            resp,
            cost_usd: call_cost,
            catalog_version: &self.catalog.version,
            node_id: &node.id,
            correlation_id: &action.correlation_id,
            settled_micros: cost,
            s0: t.trace.now_us(),
            origin: self.ran_of(&target.profile).origin,
        };
        // The call's `provider.call` row rides in its completion's frame, after
        // the node (theseus-qa0).
        let row = t.tc.rec().row(&call)?;
        let completion = Completion {
            correlation_id: action.correlation_id.clone(),
            outcome: ActionOutcome::Succeeded,
            result_ref: Some(node.id.clone()),
            external_op_id: resp.request_id.clone(),
            started_at_ms: started_ms,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("provider:{}", target.provider),
            signature: None,
            cost_micros: Some(cost),
            detail: Some(json!({"served_model": resp.model, "message_id": resp.message_id})),
        };
        if let Err(e) =
            t.tc.kernel
                .accept_completion_with(&completion, vec![node.record()?, row])
        {
            // The answer's frame was not written, though the call ended and
            // its price is known: it settles alone, at that price, so the
            // budget holds no reservation for it, and the turn faults with
            // it booked (R1). If that frame fails too, the reconcile settles
            // the call as unknown at its deadline.
            let alone = Completion {
                result_ref: None,
                detail: Some(
                    json!({"served_model": resp.model, "message_id": resp.message_id,
                                    "answer": "not written: its frame failed"}),
                ),
                ..completion
            };
            if let Err(s) = t.tc.kernel.accept_completion(&alone) {
                tracing::warn!(correlation_id = %action.correlation_id, error = %format!("{s:#}"),
                    "a provider call's spend was not settled; the reconcile settles it at its deadline");
            }
            return Err(e);
        }
        t.tc.node_written(&node);
        if !resp.text.is_empty() {
            if !t.output.is_empty() {
                t.output.push_str("\n\n");
            }
            t.output.push_str(&resp.text);
            t.said.push((i, node.id.clone()));
        }
        t.announce_fact(&call);
        if resp.stop_reason.as_deref() == Some("refusal") {
            t.record(&fact::turn::ProviderRefused { resp });
        }
        Ok(node)
    }

    /// A message's files as its node keeps them (theseus-9g2, theseus-c9l6),
    /// and a `file.read` row for each file kept. A message with a file's
    /// bytes waits on the disk and on its PDF's conversion off the runtime's
    /// workers; one without runs nothing new.
    pub(super) fn accept_files(
        &self,
        t: &mut Turn<'_>,
        wire: Vec<theseus_protocol::Attachment>,
    ) -> Vec<crate::node::Attachment> {
        let caps = crate::attach::Caps {
            max_text: self.cfg.tools.max_read_bytes,
            max_file: self.cfg.tools.max_attachment_bytes,
            page_images: self
                .cfg
                .tools
                .pdf_page_images
                .then_some(crate::page_images::POPPLER),
        };
        let blobs = self.store.blobs();
        let (files, reads) = match wire.iter().any(|w| w.data.is_some()) {
            true => theseus_store::blocking(|| crate::attach::from_wire(wire, caps, blobs)),
            false => crate::attach::from_wire(wire, caps, blobs),
        };
        for read in &reads {
            t.record(&fact::tool::FileRead { read });
        }
        files
    }

    /// A provider's 400 that names an image (theseus-0s4): mark each image it
    /// names not shown, in the session record at once and in the turn's copy,
    /// with an `image.not_shown` row each, so that this turn's next call and
    /// every later request render its line. None when it names no image the
    /// session still shows. `Some(true)` when a copy of one sat before an
    /// answer of the model's in the refused request, whose thinking the next
    /// compilation strips.
    pub(super) fn hide_refused(
        t: &mut Turn<'_>,
        session: &mut SessionRecord,
        compiled: &Compiled,
        f: &Failure,
    ) -> Result<Option<bool>> {
        let Some(ProviderError::InvalidRequest {
            status: 400,
            message,
        }) = f.source.downcast_ref::<ProviderError>()
        else {
            return Ok(None);
        };
        let messages = &compiled.request.messages;
        let refused: Vec<crate::attach::Refused> = crate::attach::refused(messages, message)
            .into_iter()
            .filter(|r| !session.not_shown.iter().any(|n| n.digest == r.digest))
            .collect();
        let Some(first) = refused.first() else {
            return Ok(None);
        };
        let at_ms = theseus_protocol::now_unix_ms();
        let marks: Vec<NotShown> = refused
            .iter()
            .map(|r| NotShown {
                digest: r.digest.clone(),
                why: r.why.clone(),
                at_ms,
            })
            .collect();
        for r in &refused {
            t.record(&fact::turn::ImageNotShown {
                refused: r,
                target: t.target,
            });
        }
        // Written now, with its rows: a restart before the turn ends keeps it.
        t.tc.store.update_session(t.tc.session_id, |rec| {
            for m in &marks {
                if !rec.not_shown.iter().any(|n| n.digest == m.digest) {
                    rec.not_shown.push(m.clone());
                }
            }
            Ok(vec![])
        })?;
        t.record(&fact::turn::ImagesHidden {
            model: &t.target.model,
            count: refused.len(),
            why: &first.why,
        });
        // Every copy of a hidden image renders as its line, not only the one
        // the 400 named. If any copy sat before an answer of the model's, the
        // history under that answer's thinking changed: the API binds a
        // thinking block to every message before it, so the block must go.
        let edited = messages
            .iter()
            .rposition(|m| m["role"] == "assistant")
            .is_some_and(|last| {
                messages[..last].iter().any(|m| {
                    crate::attach::digests_in(m)
                        .iter()
                        .any(|d| refused.iter().any(|r| &r.digest == d))
                })
            });
        session.not_shown.extend(marks);
        Ok(Some(edited))
    }

    /// The provider said this loop's request passed the window
    /// (theseus-9p88): a `context.overflow` row, and one line. The first
    /// time, the next loop recompiles with a ring by the provider's numbers
    /// and calls once more. A request that was that retry (`retried`) fails
    /// the turn instead: the turn never rings twice for one call.
    pub(super) fn overflowed(
        t: &mut Turn<'_>,
        o: Overflowing,
        retried: Option<Overflowing>,
        i: u32,
    ) -> Result<Overflowing, Failure> {
        t.record(&fact::turn::ContextOverflow {
            loop_index: i,
            overflow: &o,
            retried: retried.is_some(),
            model: &t.target.model,
        });
        if retried.is_some() {
            return Err(Self::window_failure(t, &o, true));
        }
        Ok(o)
    }

    /// A request past the window that the turn's ring cannot fit
    /// (theseus-9p88): the retry passed it too (`retried`), or the ring could
    /// drop nothing, so the same request would. The error names the window
    /// and the estimate, and says what the operator can do.
    pub(super) fn window_failure(t: &mut Turn<'_>, o: &Overflowing, retried: bool) -> Failure {
        let message = format!(
            "{}'s request is past its window{}: {}; Theseus estimated {} tokens against {}. The \
             next message lets the ring drop this exchange, or start a new session.",
            t.target.model,
            if retried {
                " again, after a ring dropped earlier turns"
            } else {
                ", and nothing earlier in the session can be dropped to fit it"
            },
            o.said(),
            narrative::thousands(o.hint.estimated),
            o.window_words()
        );
        t.record(&fact::turn::WindowFailed { retried });
        Failure {
            class: WINDOW_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: format!("{WINDOW_CLASS}: {message}"),
            source: anyhow::anyhow!(message),
        }
    }
}
