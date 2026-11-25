//! The self pipeline's answer and the switch's answer carry the same state, so `theseus self
//! pipeline` and `theseus self resume` say the same about the sealed dir: set with no set in it,
//! or not set.

use std::sync::Arc;

use theseus_protocol::rsi::{SealedState, SelfPipelineParams};

use crate::provider::FakeProvider;
use crate::store::Store;
use crate::{Config, Core};

/// A core over a throwaway state dir, with `[self.sealed] dir` set to an empty directory or not.
fn core(sealed_dir: bool) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let (state, work, sealed) = (base.join("state"), base.join("work"), base.join("sealed"));
    for d in [&state, &work, &sealed] {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    if sealed_dir {
        cfg.self_improve.sealed.dir = Some(sealed.to_string_lossy().into_owned());
    }
    cfg.validate().unwrap();
    let store = Store::open(&state.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![]));
    (
        Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap(),
        dir,
    )
}

/// KEY: a sealed dir set with no set in it: the pipeline's state says so, as the switch's does.
#[test]
fn the_pipeline_says_a_sealed_dir_is_set_with_no_set_in_it() {
    let (core, _dir) = core(true);
    let pipeline = core.self_pipeline(&SelfPipelineParams::default()).unwrap();
    assert_eq!(pipeline.state.sealed, Some(SealedState::default()));
    assert_eq!(pipeline.state, core.self_state());
}

/// KEY: no sealed dir set: the pipeline's state says none, as the switch's does.
#[test]
fn the_pipeline_says_no_sealed_dir_is_set_when_none_is() {
    let (core, _dir) = core(false);
    let pipeline = core.self_pipeline(&SelfPipelineParams::default()).unwrap();
    assert_eq!(pipeline.state.sealed, None);
    assert_eq!(pipeline.state, core.self_state());
}
