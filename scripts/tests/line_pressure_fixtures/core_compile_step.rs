
impl TurnRunner {
    /// `strip` recompiles without the prefix's thinking (theseus-0s4).
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn compile_step(
        &self,
        t: &mut Turn<'_>,
        session: &mut SessionRecord,
        spec: &RequestSpec,
        force: bool,
        strip: Option<&'static str>,
        overflowed: Option<&Overflowed>,
        i: u32,
    ) -> Result<Result<Compiled, Failure>> {
        let sid = t.tc.session_id;
        let c0 = t.trace.now_us();
        let mut ph = crate::compiler::phases::Phases::new("compile_step");
        let nodes = t.tc.store.transcript(sid)?;
        ph.mark("transcript");
        let current = match session.compilation_id.as_deref() {
            Some(id) => self.store.get_compilation(id)?,
            None => None,
        };
        // `recall_carried`, not `recall_view`: this is the turn's own compile,
        // and what its request carries decides what rides the plan frame
        // (theseus-fpm2).
        let (nodes, sources) = self.recall_carried(t, nodes);
        // Whether the provider's cache had lapsed since the session's last call: the measuring row's.
        let ttl = spec.conversation_ttl.min(spec.cache_ttl).ms();
        let cold = (i == 0).then(|| theseus_protocol::now_unix_ms() > session.last_active_ms + ttl);
        let read_before = crate::stub::read(&nodes);
        ph.mark("views");
        let given = self.situation_of(t, &nodes, current.is_none(), session);
        let input = CompileInput {
            session_id: sid,
            current: current.as_ref(),
            nodes: &nodes,
            last_position: self.store.last_position(),
            spec,
            catalog: &self.catalog,
            force,
            // `[context] working_window_tokens` lowers the window (theseus-3ki0).
            window_override: self.working_window(spec),
            blobs: Some(self.store.blobs()),
            hidden: &session.not_shown,
            strip,
            overflowed,
            sources: &sources,
            situation: &given,
        };
        // The harness's part of the block beside the message: each part rendered once a turn, before the
        // view that lays it is first assembled (`now_step`).
        if t.now.is_none() {
            let rendered = self.now_rendered(t, session).await;
            t.now = Some(rendered);
        }
        // Past the window with nothing left to drop, the turn fails before any call.
        // A place set to `view` builds it from the turn's view instead (issue 44, `view_step`).
        // A view that cannot be assembled soundly fails the turn plainly, before anything is sent.
        let mut compiled = match Box::pin(self.compile_here(t, input)).await {
            Ok(c) => c,
            Err(e) => return Self::view_failure(t, e).map(Err),
        };
        ph.mark("compile");
        Self::recall_compiled(t, &mut compiled);
        t.fill = super::fill_step::Fill::of(&compiled, self.fill_window(spec));
        Self::cleared_recorded(t, &compiled, i);
        Self::standing_listed(t, &mut compiled);
        self.defer_noted(t, spec, &compiled);
        // An await after the view's assembly and before the call is sent, as the old tail's git read
        // was: a test's seam for what is removed in between.
        #[cfg(test)]
        super::view_step::at_assembled(sid);
        // What the task graph and the project showed, as the frozen view lays them (`now_step`): a part the
        // block left out for room is not reported as shown, nor given to admission as a task view.
        let (tasks, project) = (t.now.as_ref())
            .map(|r| {
                let laid = |n| r.laid(n);
                (
                    r.tasks
                        .clone()
                        .filter(|_| laid(super::now_step::Name::Tasks)),
                    r.project
                        .clone()
                        .filter(|_| laid(super::now_step::Name::Project)),
                )
            })
            .unwrap_or_default();
        if let Some(f) = Self::overage(t, &compiled, i) {
            return Ok(Err(f));
        }
        if let Some(f) = self.admitted(t, &compiled, i)? {
            return Ok(Err(f));
        }
        ph.mark("admitted");
        // Routing may move the turn (25e): only the compilation the call
        // uses is persisted (`route_step`).
        if compiled.new_compilation && !t.route.defer_persist {
            Self::keep_blocks(t, &mut compiled);
            Self::persist_compilation(t.tc.store, &compiled, session)?;
        }
        ph.mark("persist");
        ph.log(&nodes, "");
        let c1 = t.trace.now_us();
        let mut summary = Self::compiled_summary(
            t,
            &compiled,
            spec,
            (&nodes, read_before, i),
            (tasks, project),
        );
        self.summary_done(t, &mut summary, (&nodes, &compiled, spec), cold);
        // While routing decides, the rows wait for the compile the call uses
        // (`route_step`, theseus-d13v): one `context.compiled` and one
        // `loop.started` a loop, each naming a stored compilation.
        if t.route.defer_persist {
            t.route.deferred = Some(Box::new(Deferred { summary, c0, c1 }));
            return Ok(Ok(compiled));
        }
        Self::compiled_rows(t, &summary, &compiled, spec, (c0, c1), i);
        Ok(Ok(compiled))
    }
}
