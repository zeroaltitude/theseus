//! The loop's compile step (moved out of turn.rs at cloud compaction-roots' join, M6 30c, to keep the
//! turn loop under its line ceiling): this loop's request, an append or a recompile; the task view
//! (39a); the overage check; and the `context.compiled` record.

use super::*;

/// A turn's new system blocks on their way to the blobs (theseus-nsp9l.4):
/// the texts no blob holds yet, kept back until the call's plan frame is
/// durable, then their writes, waited for once the turn's body is done.
#[derive(Default)]
pub(super) struct Keeping {
    texts: Vec<Vec<u8>>,
    writing: Vec<tokio::task::JoinHandle<()>>,
}

/// The most a turn's end waits for its blocks' writes (`blocks_kept`).
pub(super) const KEEP_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

impl Keeping {
    /// More texts to keep with the system blocks, in the same batch (a view's chain, `view_step::kept`).
    pub(super) fn add(&mut self, fresh: Vec<Vec<u8>>) {
        self.texts.extend(fresh);
    }
}

impl TurnRunner {
    /// Render the session into this loop's request: an append to the current
    /// compilation, or a recompile (persisted with the session's pointer).
    /// The images the provider refused in the session render as their line;
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
        // The owner's own file, as block 3 carries it (`own_file_step`).
        summary.context_files.extend(self.own_file_ref());
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

    /// The summary's walk tokens, and what the turn's first call is sent, for
    /// its exchange's row (issue 44).
    fn summary_done(
        &self,
        t: &mut Turn<'_>,
        summary: &mut ContextCompiled,
        (nodes, compiled, spec): (&crate::store::Transcript, &Compiled, &RequestSpec),
        cold: Option<bool>,
    ) {
        summary.walk_tokens = self.walk_tokens(spec, compiled);
        Self::exchange_sent(t, (nodes, compiled, spec), cold, summary);
    }

    /// A clearing pass (theseus-3ki0): one row a pass.
    fn cleared_recorded(t: &mut Turn<'_>, compiled: &Compiled, i: u32) {
        if let Some(pass) = compiled.cleared {
            t.record(&fact::context::Cleared {
                compilation_id: &compiled.compilation.id,
                pass,
                limit: compiled.budget.limit_tokens,
                loop_index: i,
            });
        }
    }

    /// The new compilation's system blocks, each stored once under its digest
    /// (own-benchmark): stamped here, written once the call's plan frame is
    /// durable (`start_keeping`, `cache::keep`, theseus-nsp9l.4).
    pub(super) fn keep_blocks(t: &mut Turn<'_>, compiled: &mut Compiled) {
        let texts = crate::compiler::cache::stamp(&mut compiled.compilation, &compiled.request);
        let fresh = crate::compiler::cache::fresh(t.tc.store.blobs(), &texts);
        t.keeping.texts.extend(fresh);
    }

    /// Start writing the blocks `keep_blocks` kept back, on the blocking pool.
    pub(super) fn start_keeping(t: &mut Turn<'_>) {
        let fresh = std::mem::take(&mut t.keeping.texts);
        let blobs = t.tc.store.blobs_handle();
        t.keeping
            .writing
            .extend(crate::compiler::cache::keep(&blobs, fresh));
    }

    /// Wait for the blocks' writes (the system blocks' and a view's, kept
    /// for a replay), starting any no call reached: called once the turn's
    /// body is done, so a turn that has ended has its blocks kept. The wait
    /// is bounded ([`KEEP_WAIT`]): past it the turn completes with a warning,
    /// and the writes left settle on their own without holding the session
    /// (a replay of the turn is refused, naming the missing blob, until they
    /// land).
    pub(super) async fn blocks_kept(t: &mut Turn<'_>) {
        Self::start_keeping(t);
        let until = tokio::time::Instant::now() + KEEP_WAIT;
        for writing in std::mem::take(&mut t.keeping.writing) {
            match tokio::time::timeout_at(until, writing).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "a system blocks' write did not finish; its turns cannot be replayed");
                }
                // The handle is let go: its write goes on, and nothing waits for it.
                Err(_) => {
                    tracing::warn!(
                        wait = ?KEEP_WAIT,
                        "the turn's blocks were not written in time; it completes without them, and they \
                         settle on their own"
                    );
                    return;
                }
            }
        }
    }

    /// The window a request on `spec`'s model compiles against, when
    /// `[context] working_window_tokens` lowers the catalog's.
    pub(crate) fn working_window(&self, spec: &RequestSpec) -> Option<u64> {
        let model = self.catalog.get(&spec.model).map(|e| e.context_window);
        self.cfg.context.working_window(model)
    }

    /// The loop's compile, recorded: its `context.compiled` row and
    /// `loop.started`.
    pub(super) fn compiled_rows(
        t: &mut Turn<'_>,
        summary: &ContextCompiled,
        compiled: &Compiled,
        spec: &RequestSpec,
        (c0, c1): (u64, u64),
        i: u32,
    ) {
        // Its span and its row carry the notification's params.
        t.record(&fact::turn::ContextCompiled {
            summary,
            compiled,
            spec,
            c0,
            c1,
        });
        t.record(&fact::turn::LoopStarted {
            turn_id: t.tc.turn_id,
            index: i,
            model: &t.target.model,
            tools_offered: spec.tools.len() as u32,
        });
    }

    /// The ontology walk's guidance the request carried, in tokens, counted
    /// as the census and `context.explain` count it (`context_parts`): the
    /// sections of the memberships the compile used, at the model's figures.
    /// Measured only (theseus-d9je7): no budget holds it yet.
    fn walk_tokens(&self, spec: &RequestSpec, compiled: &Compiled) -> u64 {
        let Some(walk) = spec.walk.as_deref() else {
            return 0;
        };
        let used = &compiled.compilation.manifest.memberships;
        let composition = walk.compose(&crate::ontology::recorded(used));
        let rates = self.catalog.get(&spec.model).map_or_else(
            || crate::catalog::TokenRates::of(&spec.model),
            |e| e.bytes_per_token,
        );
        (composition.sections.iter())
            .map(|s| crate::context_parts::text_tokens(s.len(), rates))
            .sum()
    }

    /// The loop's `context.compiled`: its span, its row, and the
    /// notification carry it.
    fn compiled_summary(
        t: &Turn<'_>,
        compiled: &Compiled,
        spec: &RequestSpec,
        (nodes, read_before, i): (&[(u64, crate::stub::Stub)], u64, u32),
        (tasks, project): (
            Option<theseus_protocol::tasks::TaskViewSummary>,
            Option<theseus_protocol::projects::ProjectSliceSummary>,
        ),
    ) -> ContextCompiled {
        let sid = t.tc.session_id;
        ContextCompiled {
            session_id: sid.into(),
            turn_id: t.tc.turn_id.into(),
            loop_index: i,
            decision: compiled.decision().into(),
            trigger: compiled.trigger.clone(),
            compilation_id: compiled.compilation.id.clone(),
            strategy: compiled.compilation.strategy.clone(),
            prefix_nodes: compiled.prefix_nodes as u64,
            tail_nodes: compiled.tail_nodes as u64,
            messages: compiled.messages as u64,
            est_tokens: compiled.est_tokens,
            digest: compiled.digest.clone(),
            repairs: compiled.repairs.clone(),
            tools: spec.tools.len() as u64,
            nodes_scanned: nodes.len() as u64,
            decoded: crate::stub::read(nodes) - read_before,
            stubs: nodes.len() as u64 - crate::stub::read(nodes),
            context_files: spec.context_files.clone(),
            persona: spec.persona.clone(),
            // How the compiler sized the request (theseus-f5hf).
            estimate: Some(compiled.estimate.summary()),
            // The request's cache breakpoints and their TTLs (theseus-ev1).
            cache: CacheSummary {
                breakpoints: compiled
                    .cache
                    .breakpoints()
                    .into_iter()
                    .map(String::from)
                    .collect(),
                ttl: spec.cache_ttl.as_str().into(),
                conversation_ttl: spec.conversation_ttl.min(spec.cache_ttl).as_str().into(),
                layers: crate::compiler::layers::digests_of(
                    &compiled.cache,
                    &compiled.request.system,
                )
                .into_iter()
                .collect(),
            },
            // The class of the place it speaks in, and the context files
            // that class left out (the place rule).
            class: Some(t.tc.class),
            withheld: compiled.withheld,
            // No longer computed: `continue.v1`, their reader, is retired.
            signals: Vec::new(),
            tasks,
            project,
            situation: Some(compiled.situation.clone()),
            standing_here: (t.standing.here.iter())
                .flat_map(|b| b.used.iter().map(|u| u.id.clone()))
                .collect(),
            // Counted by the step, which reads the catalog.
            walk_tokens: 0,
        }
    }
}

/// A first compile's rows, held while routing decides (theseus-d13v).
pub(super) struct Deferred {
    pub(super) summary: ContextCompiled,
    pub(super) c0: u64,
    pub(super) c1: u64,
}
