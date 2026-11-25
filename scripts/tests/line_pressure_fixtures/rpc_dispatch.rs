
impl Core {

    /// Every method, by name: each parses its params, runs, and serializes
    /// its result (`route`, `reply`).
    async fn dispatch(
        self: Arc<Self>,
        req: Request,
        tx: Outbound,
        link: &Link,
        closed: &watch::Receiver<()>,
    ) -> Result<Value, RpcFailure> {
        let arrived = std::time::Instant::now();
        let surface = link.surface;
        let conn = link.conn(&tx, closed, arrived);
        let params = req.params;
        // The MCP server's connection (step 41b): its tools' methods, and
        // the owner's acts, which their judgment refuses.
        self.mcp_admit(surface, &req.method, &params)?;
        match req.method.as_str() {
            method::HEALTH => reply(self.health_now().await),
            method::SESSION_OPEN => route(params, |p| self.session_open_on(surface, p)),
            m if super::sessions::OWN.contains(&m) => self.rpc_sessions(m, params, conn),
            m if super::retry::OWN.contains(&m) => self.rpc_retry(m, params, conn),
            method::TURN_SUBMIT => reply(self.turn_submit(parse(params)?, conn).await?),
            method::PROFILE_LIST => reply(self.profile_list()),
            method::PROFILE_USE => route(params, |p| self.profile_use(p, conn)),
            method::PROFILE_RELEASE => reply(self.profile_release(conn)?),
            method::PROFILE_PINS => reply(self.profile_pins().map_err(RpcFailure::invalid)?),
            method::PROFILE_UNPIN => route(or_empty(params), |p| self.profile_unpin(p, conn)),
            method::SESSION_HISTORY => route(params, |p| self.session_history(p)),
            // The first watch of any kind seeds the push (theseus-tq04).
            method::SESSION_WATCH => reply(self.session_watch(parse(params)?, conn).await?),
            method::SESSION_UNWATCH => route(params, |p| Ok(self.session_unwatch(p, conn))),
            method::CATALOG_LIST => reply(self.catalog_list()),
            method::COMPILATION_LIST => route(params, |p| self.compilation_list(p)),
            method::NODE_LIST => route(params, |p| self.node_list(p)),
            method::NODE_REACH => self.rpc_node_reach(params).await,
            method::ACTION_CONFIRM => route(params, |p| self.action_confirm(p, conn)),
            method::WORK_ANSWER => route(params, |p| self.work_answer(p, conn)),
            method::POLICY_TIGHTEN => route(params, |p| self.policy_tighten(p, conn)),
            method::POLICY_UNTIGHTEN => route(params, |p| self.policy_untighten(p, conn)),
            method::CONTENT_UNLABEL => route(params, |p| self.content_unlabel(p, conn)),
            method::PLACE_PUBLISH => route(params, |p| self.place_publish(p, conn)),
            method::TASK_START => route(params, |p| self.task_start_rpc(p, conn)),
            m @ (method::OWNER_PROOF_ENROLL
            | method::OWNER_PROOF_CONFIRM
            | method::OWNER_PROOF_STATUS
            | method::OWNER_PROOF_RESET
            | method::OWNER_PROOF_LOCK) => self.rpc_owner_proof(m, params, conn),
            method::HANDS_LIST => route(params, |p| self.rpc_hands_list(&p)),
            method::BUDGET_LIST | method::POLICY_EXPLAIN => self.rpc_reads(&req.method, params),
            method::ONTOLOGY_LIST
            | method::ONTOLOGY_CATEGORY_ADD
            | method::ONTOLOGY_GUIDANCE_SET
            | method::ONTOLOGY_MEMBERSHIP_SET
            | method::ONTOLOGY_PROPOSALS
            | method::ONTOLOGY_PROPOSAL_ACCEPT
            | method::ONTOLOGY_PROPOSAL_REJECT => self.rpc_ontology(&req.method, params, conn),
            method::CONFIRM_LIST => route(or_empty(params), |p| self.confirm_list_for(p, conn)),
            method::TOOL_LIST => reply(self.tool_list()),
            method::EXECUTION_LIST => route(or_empty(params), |p| self.execution_list(p)),
            method::ACTION_LIST => route(params, |p| self.action_list(p)),
            // A cancel and a stop wait for the jobs they end on the runtime's
            // timer, never holding a worker (theseus-bzq).
            method::EXECUTION_CANCEL => reply(self.execution_cancel(parse(params)?, conn).await?),
            method::EXECUTION_STOP => reply(self.execution_stop(parse(params)?, conn).await?),
            // Its filters are optional: no params lists every task.
            method::TASK_LIST => route(or_empty(params), |p| self.task_list(p)),
            m @ (method::TASK_CANCEL | TASK_NOT_THIS) => self.task_stop(m, params, conn).await,
            m @ (method::TASK_PAUSE | method::TASK_RESUME) => self.rpc_task_pause(m, params, conn),
            method::TASK_REHOME => self.rpc_task_rehome(params, conn),
            method::TASK_RESCHEDULE => self.rpc_task_reschedule(params, conn),
            method::SESSION_PROFILE => self.rpc_session_profile(params, conn),
            // Its filters are optional: no params lists every wake.
            method::WAKE_LIST => route(or_empty(params), |p| self.wake_list(p)),
            method::WAKE_CANCEL => route(params, |p| self.wake_cancel(p, conn)),
            m @ (method::LEDGER_TAIL | method::LEDGER_WINDOW) => self.ledger_reads(m, params),
            method::NARRATIVE_WATCH | method::NARRATIVE_UNWATCH if !self.narrator.on() => {
                Err(narration_off())
            }
            method::NARRATIVE_WATCH => reply(self.narrative_watch(parse(or_empty(params))?, conn)),
            method::NARRATIVE_UNWATCH => reply(self.narrative_unwatch(conn)),
            m @ (method::EXECUTIONS_WATCH | method::STATUS_SHORT) => {
                self.executions_reads(m, params, conn).await
            }
            method::EXECUTIONS_UNWATCH => reply(self.executions_unwatch(conn)),
            // A wait parks on the board's feed, never holding a worker.
            method::SESSION_WAIT => reply(self.session_wait(parse(params)?, conn).await?),
            m if super::index_rebuild::OWN.contains(&m) => self.rpc_index(m, params, conn).await,
            m @ (method::MEMORY_SEARCH | method::MEMORY_LOOKUP | method::HISTORY_READ) => {
                self.mem(m, params, conn).await
            }
            m @ (method::MEMORY_RECALLS | method::MEMORY_CITES) => self.mem_reads(m, params),
            method::MEMORY_LABEL | method::FACT_RETRACT => self.rpc_acts(&req.method, params, conn),
            // Jev's judgments (M5 23b): their rows, and one with its state.
            method::JUDGE_LIST | method::JUDGE_GET => self.rpc_reads(&req.method, params),
            // The learning ledger (M5 25c): a label, and the report.
            method::JUDGE_LABEL => route(params, |p| self.rpc_judge_label(p, conn)),
            m @ (method::LEARNING_REPORT | method::JUDGE_PROVE) => self.rpc_ledger(m, params).await,
            m if super::judge_runs::RUNS.contains(&m) => self.rpc_judge_run(m, params, conn).await,
            m if super::import::prefixed(m) => self.clone().rpc_prefixed(m, params, conn).await,
            method::BENCH_HISTORY => reply(self.bench_history(params).await?),
            method::SANDBOX_USAGE => reply(self.sandbox_usage()),
            method::MCP_LIST => reply(self.mcp.list(&self.tools)),
            m @ (method::EXTEND_LIST | method::EXTENSION_REVOKE) => self.rpc_ext(m, params, conn),
            method::MCP_RESTART => reply(self.mcp_restart(parse(params)?)?),
            method::MCP_PROMPT_LIST => reply(self.mcp_prompt_list(params)?),
            // AWS's bootstrap (C2): the plan reads; the apply waits for the stacks.
            method::AWS_BOOTSTRAP => reply(self.aws_bootstrap(parse(params)?, conn).await?),
            method::AWS_CONFIRM_ALERTS => {
                reply(self.aws_confirm_alerts(parse(params)?, conn).await?)
            }
            // The loops wake once the answer is written (`serve_connection`).
            method::SHUTDOWN => reply(self.stopping()),
            other => Err(RpcFailure::new(
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {other:?}"),
            )),
        }
    }
}
