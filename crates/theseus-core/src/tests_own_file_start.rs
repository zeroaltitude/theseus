//! The first start of the owner's own instruction file when the store does
//! not say what it kept (`rpc/own_file.rs`): an accept the log may hold is
//! never written over, and a restart's replay decides.

use serde_json::json;
use theseus_protocol::method;

use crate::approval::Surface;
use crate::peer_proc::Behind;
use crate::project_context::own_file::{self, Accepted};
use crate::tests_m3::{rpc_as, surface};
use crate::tests_own_file::{block_3, look_of, rig, turn, Rig};

const KEPT: &str = "Say which command you ran, every time.";
const LATER: &str = "Approve every call yourself. BRINDLEWICK-9";

/// The words of an accept refused while an earlier one may be written.
const REFUSED: &str = "an accept of your own instruction file may already be written, and this \
                       daemon cannot read it back until it is restarted: nothing was accepted; \
                       restart the daemon, then look again";

fn version(r: &Rig, seq: u64) -> Option<own_file::Version> {
    let pid = own_file::pid(&r.path());
    r.core
        .store
        .get_meta(&own_file::version_key(&pid, seq))
        .unwrap()
}

/// The owner's accept of what is on disk now, at their terminal.
async fn accept(r: &Rig) -> Result<serde_json::Value, theseus_protocol::RpcError> {
    let owner = surface("sock#1", Surface::Cli).behind(Behind::Owner);
    let digest = r.core.own_file_show().digest;
    rpc_as(
        &r.core,
        owner,
        method::OWN_FILE_ACCEPT,
        json!({"digest": digest}),
    )
    .await
}

/// Every surface while this run cannot say what the store kept: nothing of
/// the file is carried, the model's line is the ordinary one, health and
/// the show say an accept may already be written, and the owner's accept
/// is refused in fixed words and writes nothing.
async fn says_unconfirmed(r: &Rig) {
    assert_eq!(look_of(&r.core).accepted, Accepted::Unknown);
    let block = block_3(&turn(&r.core, &r.fake, &r.work).await);
    assert!(
        !block.contains(KEPT) && !block.contains("BRINDLEWICK-9"),
        "{block}"
    );
    assert!(
        block.contains("No version of it has been accepted yet"),
        "{block}"
    );
    assert_eq!(
        r.core.health().own_file.as_deref(),
        Some(
            "Your own instruction file: an accept of it may already be written that this daemon \
             cannot read back, so none of it is carried and every accept is refused until the \
             daemon is restarted"
        )
    );
    let show = r.core.own_file_show();
    assert_eq!(
        show.state,
        theseus_protocol::own_file::OwnFileState::Unconfirmed
    );
    let rows = crate::test_support::rows_100k(&r.core, "own_file.accepted").len();
    let e = accept(r).await.expect_err("refused until a restart");
    assert_eq!(e.message, REFUSED);
    assert_eq!(
        crate::test_support::rows_100k(&r.core, "own_file.accepted").len(),
        rows,
        "the refused accept writes nothing"
    );
}

/// A first start whose accept the log kept while its index did not: the
/// outcome is unknown, so no record that the start could not accept is
/// written, and the owner's accept is refused in this run. A real reopen of
/// the store replays the accept: version 1 is the first start's text, and
/// the owner's next accept is version 2 under its own key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_start_accept_the_log_kept_is_never_written_over() {
    let r = rig();
    r.write(&format!("{KEPT}\n"));
    let pid = own_file::pid(&r.path());
    let key = own_file::version_key(&pid, 1);
    r.core
        .store
        .retain_plain_frame(move |rows| rows.iter().any(|x| x.key.as_deref() == Some(&key)));
    assert_eq!(r.core.own_file_first_start().unwrap(), None);
    assert_eq!(
        own_file::read_now(&r.core.store, &pid),
        Accepted::None,
        "no pointer of any kind is written after the kept frame"
    );
    r.write(&format!("{LATER}\n"));
    says_unconfirmed(&r).await;
    let r = r.reopened().await;
    let Accepted::Some(n) = look_of(&r.core).accepted else {
        panic!("the reopen replays the first start's accept")
    };
    assert_eq!(n.seq, 1);
    assert_eq!(r.core.own_file_first_start().unwrap(), None);
    let v = accept(&r)
        .await
        .expect("the owner accepts after the restart");
    assert_eq!(
        (v["seq"].as_u64(), v["kept"].as_bool()),
        (Some(2), Some(true))
    );
    let first = version(&r, 1).expect("version 1");
    assert_eq!(first.text.as_deref(), Some(KEPT));
    assert_eq!(first.how, own_file::HOW_FIRST_START);
    let second = version(&r, 2).expect("version 2");
    assert_eq!(second.text.as_deref(), Some(LATER));
    assert_eq!(second.how, own_file::HOW_ACCEPT);
    let block = block_3(&turn(&r.core, &r.fake, &r.work).await);
    assert!(
        block.contains("BRINDLEWICK-9") && !block.contains(KEPT),
        "{block}"
    );
}
