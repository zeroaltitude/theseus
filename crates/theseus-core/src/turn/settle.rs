//! How a turn ends: a failed or cut loop settled, the tools a response proposed run, the turn failed or faulted, and
//! the frame that finishes it (its end note, its compilation, its attributes).

use super::*;

impl TurnRunner {
    /// Settle a call that failed: as failed, or as unknown when the provider
    /// may have done the work, and tell the ledger why.
    pub(super) fn settle_failed(
        t: &mut Turn<'_>,
        action: &Action,
        started_ms: u64,
        call_started: Instant,
        e: anyhow::Error,
        i: u32,
    ) -> Failure {
        let target = t.target;
        let pe = e.downcast_ref::<ProviderError>();
        let (class, transient, unknown) = pe
            .map(|p| (p.class(), p.is_transient(), p.usage_unknown()))
            .unwrap_or(("unknown", false, true));
        t.record(&fact::turn::ModelCallFailed {
            class,
            message: e.to_string(),
        });
        let s0 = t.trace.now_us();
        let settled = t.tc.kernel.accept_completion(&Completion {
            correlation_id: action.correlation_id.clone(),
            outcome: if unknown {
                ActionOutcome::Unknown
            } else {
                ActionOutcome::Failed
            },
            result_ref: None,
            external_op_id: None,
            started_at_ms: started_ms,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("provider:{}", target.provider),
            signature: None,
            cost_micros: if unknown { None } else { Some(0) },
            detail: Some(json!({"class": class})),
        });
        t.record(&fact::turn::ProviderError {
            loop_index: i,
            target,
            class,
            transient,
            unknown,
            elapsed_ms: call_started.elapsed().as_millis() as u64,
            detail: pe.map(|p| serde_json::to_value(p).unwrap_or(Value::Null)),
            message: e.to_string(),
            correlation_id: &action.correlation_id,
            reserved_micros: action.reserved_micros,
            settled: settled
                .as_ref()
                .map(|a| format!("{a:?}"))
                .unwrap_or_else(|e| e.to_string()),
            s0,
        });
        Failure {
            class: class.into(),
            transient,
            usage_unknown: unknown,
            reason: format!("provider:{class}"),
            source: e,
        }
    }

    /// A `/stop` cut the model's call (theseus-yey): the stream is dropped, the
    /// turn acts on nothing it said, and no answer node is written. The call
    /// is settled as failed at an estimate of what it used, its reservation
    /// released and nothing held unknown: the provider's own usage events when
    /// the stream had sent one (theseus-k48c), else the input estimate its reservation
    /// used, split as its request was sent (`cut_step`: the previous call's
    /// cache at the read price, theseus-3p36), and its output from the
    /// characters streamed (`CHARS_PER_TOKEN` to a token), at the model's
    /// prices, never more than it reserved. A stop
    /// before anything was sent used nothing. The `provider.cut` row says
    /// whether it is an estimate (`estimated`) and how its input was split
    /// (`split`, `provider_events` for the provider's own count). The output
    /// the provider had not yet reported stays an estimate from the characters
    /// streamed, so a call cut early is booked a floor, never more than it
    /// reserved.
    pub(super) fn settle_cut(
        t: &mut Turn<'_>,
        action: &Action,
        price: &crate::catalog::CatalogEntry,
        cut: Cut,
        started_ms: u64,
        call_started: Instant,
        i: u32,
    ) -> Called {
        let target = t.target;
        let streamed = cut.out_chars.div_ceil(CHARS_PER_TOKEN) * u64::from(cut.sent);
        // The provider's own usage events, when the stream had sent one, beat the
        // estimate: its prompt as it counted it, and the larger of the output it
        // had reported and the characters streamed (theseus-k48c).
        let (usage, split, estimated) = match (&cut.seen, cut.sent) {
            // The prompt is the provider's, always; the booking is wholly its own only when what it
            // had reported of the output was not less than the characters streamed say.
            (Some(seen), _) => (
                Usage {
                    output_tokens: seen.output_tokens.max(streamed),
                    ..seen.clone()
                },
                cut_step::SPLIT_PROVIDER,
                streamed > seen.output_tokens,
            ),
            (None, true) => {
                let (usage, split) = cut.prefix.split(cut.est_input);
                (
                    Usage {
                        output_tokens: streamed,
                        ..usage
                    },
                    split,
                    true,
                )
            }
            (None, false) => (Usage::default(), cut_step::SPLIT_UNCACHED, true),
        };
        let cost = price
            .cost_micros_at(&usage, started_ms)
            .min(action.reserved_micros);
        t.record(&fact::turn::ModelCallFailed {
            class: "stopped",
            message: format!("cut by a stop from {}", cut.by),
        });
        let s0 = t.trace.now_us();
        let settled = t.tc.kernel.accept_completion(&Completion {
            correlation_id: action.correlation_id.clone(),
            outcome: ActionOutcome::Failed,
            result_ref: None,
            external_op_id: None,
            started_at_ms: started_ms,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("provider:{}", target.provider),
            signature: None,
            cost_micros: Some(cost),
            detail: Some(
                json!({"class": "stopped", "by": cut.by, "estimated": estimated,
                "input_tokens": usage.input_tokens, "output_tokens": usage.output_tokens,
                "cache_read_input_tokens": usage.cache_read_input_tokens,
                "cache_creation_input_tokens": usage.cache_creation_input_tokens,
                "split": split, "output_chars": cut.out_chars}),
            ),
        });
        t.cost = t.cost.map(|c| c + micros_to_usd(cost));
        add_usage(&mut t.usage, &usage);
        t.record(&fact::turn::ModelCut {
            loop_index: i,
            target,
            correlation_id: &action.correlation_id,
            by: &cut.by,
            sent: cut.sent,
            usage: &usage,
            split,
            estimated,
            output_chars: cut.out_chars,
            cost,
            reserved_micros: action.reserved_micros,
            elapsed_ms: call_started.elapsed().as_millis() as u64,
            settled: settled
                .as_ref()
                .map(|a| format!("{a:?}"))
                .unwrap_or_else(|e| e.to_string()),
            s0,
        });
        Called::Stopped { by: cut.by }
    }

    /// Gate the model's tool calls in order, until one waits on a confirm,
    /// and run the ones before it, the reads of a group together
    /// (`ToolRuntime::run_calls`, theseus-a60). A response that did not stop
    /// for tools runs none of them. Returns how many calls have an answer.
    pub(super) async fn run_tools(
        &self,
        t: &mut Turn<'_>,
        resp: &ModelResponse,
        uses: &[ToolUse],
        node: &Node,
        i: u32,
    ) -> Result<u32> {
        let tc = TurnCtx {
            loop_index: Some(i),
            ..t.tc
        };
        let stop = resp.stop_reason.as_deref();
        if stop != Some("tool_use") {
            let why = format!(
                "the response ended with stop reason `{}`",
                stop.unwrap_or("none")
            );
            for u in uses {
                self.tools.not_run(&tc, u, &why)?;
            }
            return Ok(0);
        }
        // One id on more than one call of the response: none of them runs, judged on the WHOLE response before
        // anything is deferred, fitted or gated (issue 44).
        let (uses, refused) = self.tools.refuse_repeated(&tc, &node.id, uses)?;
        // A call to a tool the request did not load is answered, not run.
        let (uses, unloaded) = self.defer_unloaded(t, &tc, &uses)?;
        // The calls as the model meant them (theseus-9dt2).
        let (uses, recovered) = self.tools.fit_uses(&uses, &resp.invalid_tool_inputs);
        let uses = uses.as_slice();
        let calls: Vec<Call<'_>> = uses
            .iter()
            .map(|call| Call {
                call,
                invalid: (!recovered.contains(&call.id))
                    .then(|| resp.invalid_tool_inputs.get(&call.id).map(String::as_str))
                    .flatten(),
            })
            .collect();
        let batch = self.tools.run_calls(&tc, &node.id, &calls).await?;
        t.tool_calls += batch.ran.len() as u32;
        calls::trace_calls(&mut t.trace, &self.tools, uses, &batch.ran);
        let mut answered = 0;
        // One status per judged call, for the repeat cap (`repeat_step`).
        let mut outcomes = Vec::with_capacity(batch.ran.len());
        for r in batch.ran {
            t.checkpoint.answered(&uses[r.index], &r.outcome);
            match r.outcome {
                CallOutcome::AwaitingConfirm { .. } => {}
                CallOutcome::Background { correlation_id } => {
                    t.background.push(correlation_id);
                    outcomes.push(ResultStatus::Background);
                    answered += 1;
                }
                CallOutcome::Done { status } => {
                    outcomes.push(status);
                    answered += 1;
                }
            }
        }
        t.awaiting = batch.awaiting;
        t.repeats.note_idle(uses, &outcomes, t.awaiting.is_some());
        Ok(answered + unloaded + refused)
    }

    /// End a turn that failed after it began, the one exit of every turn
    /// that does not finish (R1): book what its loops spent, close its trace,
    /// and write `turn.failed`. What the end had already done (`finish`, then
    /// a fault) is not done twice: books counted once, one accounting row
    /// (`turn.ended` or `turn.failed`), and the session's write once its last
    /// frame carries it.
    pub(super) fn fail(mut t: Turn<'_>, session: &mut SessionRecord, f: Failure) -> anyhow::Error {
        t.close_books(session);
        if !t.ended {
            t.tc.record(&fact::turn::TurnFailed {
                turn_id: t.tc.turn_id,
                class: &f.class,
                reason: &f.reason,
                loops: t.loops,
                usage: &t.usage,
                cost_usd: t.cost,
                tool_calls: t.tool_calls,
            });
            super::exchange_step::failed(&mut t, &f.class);
        }
        let trace = t
            .trace
            .finish(json!({"outcome": "failed", "class": f.class}));
        if !t.deferred {
            let written = t.tc.store.update_session(t.tc.session_id, |r| {
                r.take_turns_fields(session);
                Ok(vec![])
            });
            if let Err(e) = written {
                tracing::warn!(session_id = %t.tc.session_id, turn_id = %t.tc.turn_id,
                    error = %format!("{e:#}"), "a failed turn's books were not written to its session");
            }
        }
        TurnError {
            class: f.class,
            transient: f.transient,
            usage_unknown: f.usage_unknown,
            turn_id: t.tc.turn_id.into(),
            session_id: t.tc.session_id.into(),
            elapsed_ms: t.started.elapsed().as_millis() as u64,
            trace: Some(trace),
            usage: t.usage,
            cost_usd: t.cost,
            tool_calls: t.tool_calls,
            source: f.source,
        }
        .into()
    }

    /// A fault: an error the turn did not report itself, from any step after
    /// it began (R1). It is logged, and ends the turn as a failure of class
    /// `internal` does, so the session, the `turn.failed` row, telemetry,
    /// and the error itself carry what the turn's loops spent.
    pub(super) fn fault(
        t: Turn<'_>,
        session: &mut SessionRecord,
        e: anyhow::Error,
    ) -> anyhow::Error {
        tracing::warn!(session_id = %t.tc.session_id, turn_id = %t.tc.turn_id,
            error = %format!("{e:#}"), "the turn faulted; its books are closed");
        let reason = crate::toolrun::cap(
            &format!("{FAULT_CLASS}: {e:#}"),
            400,
            |_| String::new(),
            &t.tc.scrubber.cuts(),
        )
        .0;
        Self::fail(
            t,
            session,
            Failure {
                class: FAULT_CLASS.into(),
                transient: false,
                usage_unknown: false,
                reason,
                source: e,
            },
        )
    }

    /// The turn's root span's closing attributes: its frames, and the slowest
    /// frame the store's writer answered since its input arrived
    /// (theseus-w7dk), for `bench turn` to name.
    fn end_attrs(t: &Turn<'_>, result: &TurnSubmitResult, frames: Option<u64>) -> Value {
        json!({
            "outcome": "complete",
            "loops": result.loops,
            "stop_reason": result.stop_reason,
            "usage": result.usage,
            "frames": frames,
            "slowest_frame": t.tc.store.slowest_frame_since(t.arrived),
        })
    }

    /// Absorb results that settled while the turn ran, book the turn, write
    /// its result, and park the execution. The `bool` asks for another turn:
    /// a late result the model has not read. The session write waits for
    /// the turn's last frame, `end_turn`'s, under the returned hold
    /// (theseus-l6y). A fault at any step is the turn's one exit (`fault`),
    /// which books what this has not.
    pub(super) fn finish(
        &self,
        mut t: Turn<'_>,
        session: &mut SessionRecord,
    ) -> Result<(TurnSubmitResult, TurnEnd, bool, Option<SessionHold>)> {
        let jobs = self.jobs_before(&t);
        let late = match self.take_late(&mut t) {
            Ok(late) => late,
            Err(e) => return Err(Self::fault(t, session, e)),
        };
        let end = match self.end_of(&t) {
            Ok(end) => end,
            Err(e) => return Err(Self::fault(t, session, e)),
        };
        t.close_books(session);
        session.last_target = Some(Self::ran_on(&t));
        let w0 = t.trace.now_us();
        // The labels the session carries once this turn's are merged in
        // (theseus-emqbm), read under the record's lock where that merge
        // happens, so a label this turn raised is on the reply that carries
        // it. No store read of its own; the words are `content::footer_labels`.
        let mut labels = vec![];
        // Only the turn's own fields: a change written meanwhile stays
        // (theseus-xeo). The record rides in the turn's last frame, where
        // the rows before it here and after it wait too; the span times its
        // making, and the frame's write is `end_turn`'s.
        // Its replies, each asked the content questions off the path (issue 44), queued BEFORE the hold
        // below is taken: a configured name's label lands at once, under the session's lock.
        self.content_replies(&t);
        let hold = match t.tc.store.defer_session(t.tc.session_id, |r| {
            r.take_turns_fields(session);
            labels = footer_labels(&self.cfg.cockpit, t.tc.session_id, r.data.as_deref());
        }) {
            Ok(hold) => hold,
            Err(e) => return Err(Self::fault(t, session, e)),
        };
        t.deferred = true;

        let cites = self.cite_reply(&mut t);
        let last = t.last.as_ref();
        let mut result = TurnSubmitResult {
            session_id: t.tc.session_id.into(),
            turn_id: t.tc.turn_id.into(),
            loops: t.loops,
            output: std::mem::take(&mut t.output),
            stop_reason: if t.stop_reason.is_empty() {
                "end_turn".into()
            } else {
                t.stop_reason.clone()
            },
            provider_stop_reason: last.and_then(|r| r.stop_reason.clone()),
            model: last.map_or_else(|| t.target.model.clone(), |r| r.model.clone()),
            provider: t.target.provider.clone(),
            profile: t.target.profile.clone(),
            usage: t.usage.clone(),
            elapsed_ms: t.started.elapsed().as_millis() as u64,
            first_token_ms: last.and_then(|r| r.timing.first_token_ms),
            request_id: last.and_then(|r| r.request_id.clone()),
            trace: None,
            execution_id: Some(t.tc.execution_id.into()),
            cost_usd: t.cost,
            // The record in hand, its books closed above: no store read
            // (theseus-c0bb), and the total `session.list` will answer.
            session_cost_usd: Some(session.cost_usd),
            context_tokens: t.fill.tokens,
            context_window: t.fill.window,
            tool_calls: t.tool_calls,
            awaiting_confirm: t.awaiting.clone().or_else(|| t.budget_question.clone()),
            stop_details: last.and_then(|r| r.stop_details.clone()),
            continuation: t.continuation,
            recalled: t.recall.count,
            route: t.route.result.take(),
            fallback: t.fallback.as_ref().map(|f| f.fallback.clone()),
            later: self.later_of(&t, jobs, late > 0 && t.stop_reason != "stopped"),
            outside_roots: None,
            nudged: t.nudge.result(t.tool_calls),
            files: self.files_left(&t),
            raised: None,
            cites,
            thread: None,
            filed: None,
            inspect_url: self.cfg.cockpit.turn_link(t.tc.session_id, t.tc.turn_id),
            labels,
        };
        result.raised = self.defer_ended(&mut t);
        self.initiative_rows(&mut t, &result);
        t.record(&fact::turn::TurnBooked {
            result: &result,
            session_usage: &session.usage,
            late,
            w0,
        });
        t.ended = true;
        // A task's turns post nothing to the place: its report does, once,
        // when it ends (DD7). A stopped turn posts nothing of its own either (W1): the
        // stop's answer says what stopped; an answer another profile gave it is still shown.
        if t.tc.task.is_none() {
            if let Err(e) = self.stage_end_post(&t, &result, session.failing.as_ref()) {
                return Err(Self::fault(t, session, e));
            }
        }
        // The turn's frames (theseus-wz4y): those written so far, and its
        // last, `end_turn`'s, which carries this trace. The bench counts the
        // same turn's from the WAL (`theseus-sim bench turn`).
        let frames = t.tc.store.turn_frames().map(|n| n + 1);
        // A judgment of this turn's end is marked before its last frame (23b).
        self.mark_end(&mut t, &result);
        // Its exchange's end and its measuring row, in the last frame (issue 44).
        self.exchange_close(&mut t, &result);
        let attrs = Self::end_attrs(&t, &result, frames);
        result.trace = Some(t.trace.finish(attrs));
        // The trace is finished: the fact draws no span.
        t.tc.record(&fact::turn::TurnEnded { result: &result });
        let is_task = t.tc.task.is_some();
        let stop = result.stop_reason.clone();
        // A stopped turn takes no other by itself (W1), even for a result
        // that landed during it: the next input's turn reads it.
        let mut again = late > 0 && stop != "stopped";
        // A task that has nothing left to wait on is done (DD7): its last
        // message is its report, which the frame that ends it carries. One
        // with results its model has not read (late ones, or a turn stopped at
        // its loop cap) takes another turn instead: no one else will wake it.
        // One with a wake pending parks on it, and the wake's turn goes on (37b).
        let end = self.task_end(
            (t.tc.kernel, t.tc.execution_id, t.tc.session_id),
            end,
            is_task,
            &mut again,
            &stop,
        );
        Ok((result, end, again, hold))
    }

    /// A new compilation (it carries its own `derived_from`) and the
    /// session's pointer, in one frame; a view turn's in frames the turn
    /// writes anyway. The stored record takes only the turn's fields
    /// (theseus-xeo).
    pub(super) fn persist_compilation(
        store: &Store,
        compiled: &Compiled,
        session: &mut SessionRecord,
    ) -> Result<()> {
        let c = &compiled.compilation;
        session.compilation_id = Some(c.id.clone());
        let records = || -> Result<Vec<NewRecord>> {
            // The compilation alone: its `context.recompiled` row is no longer written (issue 46), since
            // the turn's `context.compiled` row names the same compilation. Old rows still read
            // (`fact::turn::ContextRecompiled`, `LedgerKind::ContextRecompiled`).
            Ok(vec![NewRecord::json(
                theseus_store::kinds::COMPILATION,
                Some(&c.id),
                c,
            )?
            .scoped(&c.session_id)])
        };
        // A view turn builds a compilation every turn (issue 44, issue 46): its records ride the
        // turn's next frame, and the pointer rides the turn's own session write at its end
        // (`take_turns_fields`), so neither costs a frame of its own. A session never stored, or
        // one this turn titles, takes the path below: a card the turn asks names the session by
        // the title the store holds (`needs_you::asker_name`).
        let titled = store
            .get_session::<SessionRecord>(&session.session_id)?
            .is_some_and(|r| r.title == session.title);
        if c.strategy == crate::compiler::view::STRATEGY && titled {
            return records()?.into_iter().try_for_each(|r| store.defer(r));
        }
        let written = store.update_session(&session.session_id, |r| {
            r.take_turns_fields(session);
            records()
        })?;
        if written.is_none() {
            // A session whose record was never stored: this is its first.
            let mut frame = records()?;
            frame.push(NewRecord::json(
                theseus_store::kinds::SESSION,
                Some(&session.session_id),
                &*session,
            )?);
            store.append(&frame)?;
        }
        Ok(())
    }
}
