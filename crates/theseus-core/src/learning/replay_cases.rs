//! `theseus judge replay --cases` (theseus-h66v4, row 1): the red commit's skeleton. The run is
//! not built yet, so the tests written for it fail.

use std::sync::Arc;

use anyhow::bail;
use theseus_protocol::judge_runs::{JudgeReplayParams, JudgeReplayResult};

use crate::rpc::Core;

pub(crate) fn ignored_with_cases(_p: &JudgeReplayParams) -> Option<String> {
    None
}

impl Core {
    pub(super) async fn cases_replay_run(
        self: &Arc<Self>,
        _p: JudgeReplayParams,
        _who: crate::approval::Answerer,
    ) -> anyhow::Result<JudgeReplayResult> {
        bail!("--cases is not built yet")
    }
}
