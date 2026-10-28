//! Old record layouts still read (theseus-ptx1; P5b, theseus-qa0 F4a).
//!
//! One table: a sample of each record kind as each older layout a store may
//! still hold wrote it. Each is literal bytes an older build wrote, never
//! bytes today's encoder made, which prove nothing about old ones
//! (theseus-djfj): the 460a35b fixture's records (`tests/fixtures`, every
//! kind at schema 1), and the layouts since, each as its build wrote it. For
//! each, this build's reader for its kind:
//! - decodes it;
//! - keeps every field of it, byte for byte, but the ones this build no
//!   longer has, which the sample names;
//! - encodes it, and decodes that, to the same bytes: a round trip loses
//!   nothing.
//!
//! The store has one format number (`MANIFEST_FORMAT`): a step that adds a
//! field to a stored record bumps it, so an older build refuses the newer
//! store, and adds here a sample of the layout it replaces when that layout
//! is on disk somewhere. A layout is named by its kind's schema number when
//! the store kept one per kind; those numbers are frozen.

use serde_json::Value;
use theseus_kernel::{Action, Completion, Execution};
use theseus_store::{kinds, RecordKind};

use crate::compiler::Compilation;
use crate::ledger::LedgerRow;
use crate::node::Node;
use crate::session::SessionRecord;

/// What this build's encoding of a sample keeps.
enum Kept {
    /// Every byte.
    All,
    /// Every field but these (JSON pointers), which this build no longer has.
    AllBut(&'static [&'static str]),
}

struct Sample {
    kind: RecordKind,
    /// Which layout: the kind's schema number then, and what came after it.
    layout: &'static str,
    bytes: &'static str,
    kept: Kept,
}

/// A plan item as format 14 wrote it (39a, theseus-ext.6), before the
/// model's mark on its origin (`origin.by_model`, format 16, theseus-ext.10):
/// it reads as the owner's, so a change to its layer 1 waits
/// (`tests_task_layers`). By hand, in the layout the build before
/// theseus-ext.10 (1bb9b9b) writes.
pub(crate) const TASK_BEFORE_MODEL_MARK: &str = r#"{"id":"tsk_00000000000000000000000000000091","version":1,"title":"Chart the harbour soundings","objective":"chart every buoy of the outer harbour","acceptance":["every buoy has a depth on the chart"],"state":"accepted","deps":[],"owner":"agent","origin":{"session":"ses_lighthouse","principal":"operator"},"evidence":[{"node":"msg_00000000000000000000000000000090","identity":"commit:0a1b2c3d","by":"ses_lighthouse","at_ms":1790000000090}],"created_at_ms":1790000000091,"updated_at_ms":1790000000091}"#;

/// A plan item as format 16 wrote it (theseus-ext.10), before a task's
/// claim (`claim`, format 19, M7 39b, theseus-ext.14): it reads unclaimed,
/// free for any session's `task.claim`. By hand, in the layout the build
/// before 39b (3085f71) writes.
pub(crate) const TASK_BEFORE_CLAIM: &str = r#"{"id":"tsk_00000000000000000000000000000095","version":2,"title":"Chart the reef","objective":"chart the reef's north edge","acceptance":["every marker has a depth"],"state":"accepted","deps":[],"owner":"agent","origin":{"session":"ses_lighthouse","principal":"operator","by_model":true},"evidence":[],"created_at_ms":1790000000095,"updated_at_ms":1790000000096}"#;

/// A session record as format 24 wrote it (theseus-anh3's build, unchanged
/// since format 23), before a session's state (format 25, theseus-emqx): a
/// place's bind session that never took a turn, as a Discord bind opened
/// one. It reads with no retirement, no links and no old title, and
/// derives empty-retired past its grace (`rpc::tests_session_states`). By
/// hand, in the layout the build before theseus-emqx (73f0a3e) writes.
pub(crate) const SESSION_BEFORE_STATES: &str = r#"{"session_id":"ses_00000000000000000000000000000071","kind":"conversation","label":"discord #lighthouse","created_at_unix_ms":1790000000071,"turns":0,"last_turn_id":null,"usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"execution_id":"exe_00000000000000000000000000000071","last_active_ms":1790000000071,"cost_usd":0.0,"tool_calls":0}"#;

/// A category as format 24 wrote it (theseus-anh3's build, unchanged
/// through 25), an `onto:cat:` META record: an import's topic, before a
/// person's handles and merge (`handles`, `merged_into`, format 26,
/// theseus-wy7y). It reads with no handles and merged into nothing. By hand,
/// in the layout the build before theseus-wy7y (78b58b8) writes.
pub(crate) const CATEGORY_BEFORE_PEOPLE: &str = r#"{"id":"topic:garden-beds","name":"beds","parent":"topic:garden","description":"imported label garden/beds","added_by":"import tern-2026-02"}"#;

/// A session record as format 26 wrote it (theseus-wy7y's build; its
/// layout unchanged since 25), before a session's directory (`dir`, format
/// 27, theseus-aab7): a conversation the owner retired by hand and reopened,
/// re-titled once. It reads with no directory, so its tools work in
/// `[tools] cwd` as before. By hand, in the layout the build before
/// theseus-aab7 (2fd1f65) writes.
pub(crate) const SESSION_BEFORE_DIR: &str = r#"{"session_id":"ses_00000000000000000000000000000261","kind":"conversation","label":null,"created_at_unix_ms":1790000000261,"turns":3,"last_turn_id":"turn_00000000000000000000000000000263","usage":{"input_tokens":1500,"output_tokens":310,"cache_read_input_tokens":900,"cache_creation_input_tokens":200},"execution_id":"exe_00000000000000000000000000000261","compilation_id":"cmp_00000000000000000000000000000262","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790000000269,"cost_usd":0.0142,"tool_calls":2,"title":"Chart the tide gauges","retired":{"reason":"by_hand","at_ms":1790000000265},"reopened_ms":1790000000267,"title_was":["Read the harbour log"]}"#;

/// A session record as format 29 wrote it (theseus-a7vi's build; unchanged
/// since 28, theseus-aab7), before a fork's origin (`forked_from`, format
/// 32, theseus-2qzx): a conversation that works in a directory of its own.
/// It reads with no origin, so its transcript is its own nodes alone. By
/// hand, in the layout the build before theseus-2qzx (7005d130) writes.
pub(crate) const SESSION_BEFORE_FORK: &str = r#"{"session_id":"ses_00000000000000000000000000000291","kind":"conversation","label":null,"created_at_unix_ms":1790000000291,"turns":2,"last_turn_id":"turn_00000000000000000000000000000293","usage":{"input_tokens":800,"output_tokens":120,"cache_read_input_tokens":300,"cache_creation_input_tokens":100},"execution_id":"exe_00000000000000000000000000000291","compilation_id":"cmp_00000000000000000000000000000292","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790000000299,"cost_usd":0.0081,"tool_calls":1,"title":"Chart the harbour soundings","dir":"/work/harbour"}"#;

/// A compilation as format 29 wrote it (theseus-a7vi's build), before a
/// compilation's inherited run (`inherits`, format 32, theseus-2qzx): its
/// manifest lists a standing record. It reads with no run, so every node it
/// includes is named in `includes`. By hand, in the layout the build before
/// theseus-2qzx (7005d130) writes.
pub(crate) const COMPILATION_BEFORE_FORK: &str = r#"{"id":"cmp_00000000000000000000000000000292","schema":1,"session_id":"ses_00000000000000000000000000000291","created_at_ms":1790000000292,"trigger":"new_session","strategy":"transcript","as_of":71,"includes":["msg_00000000000000000000000000000291"],"derived_from":null,"manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read"],"catalog_version":"2026-10-01","context_window":1000000,"strip_thinking":false,"standing":[{"id":"std_00000029","shown":true}]},"situation":{"kind":"conversation_start"}}"#;

/// A compilation as format 32 wrote it (theseus-2qzx's build), before a
/// manifest's loaded tool families (`loaded`, format 36, theseus-7m7w.2): its
/// request carried every tool. It reads with none loaded recorded, so an append
/// to it keeps every tool and its prefix is unchanged. By hand, in the layout
/// the build before theseus-7m7w.2 writes.
pub(crate) const COMPILATION_BEFORE_LOADED: &str = r#"{"id":"cmp_00000000000000000000000000000332","schema":1,"session_id":"ses_00000000000000000000000000000331","created_at_ms":1790000000332,"trigger":"new_session","strategy":"transcript","as_of":83,"includes":["msg_00000000000000000000000000000331"],"derived_from":null,"manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read","proc_run"],"catalog_version":"2026-10-13","context_window":1000000,"strip_thinking":false},"situation":{"kind":"conversation_start"}}"#;

/// A compilation as format 33 wrote it, a thread's (theseus-h4a7's layers in
/// its cache layout), before the prefix library named a header in its
/// manifest (`prefix`, format 41, theseus-rr15): it reads with none, and its
/// session's next compile point picks one.
pub(crate) const COMPILATION_BEFORE_PREFIX: &str = r#"{"id":"cmp_00000000000000000000000000000341","schema":1,"session_id":"ses_00000000000000000000000000000341","created_at_ms":1790000000341,"trigger":"cold_resume","strategy":"ring","as_of":91,"includes":["msg_00000000000000000000000000000342"],"derived_from":"cmp_00000000000000000000000000000340","manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read","thread_note"],"catalog_version":"2026-10-01","context_window":1000000,"strip_thinking":true,"cache":{"caches":true,"min_tokens":1024,"blocks":[{"block":"header","prefix_bytes":21000,"marked":true},{"block":"persona","prefix_bytes":24000,"marked":true},{"block":"project","prefix_bytes":31000,"marked":true},{"block":"session","prefix_bytes":31060,"marked":false}]}},"situation":{"kind":"recompile","trigger":"cold_resume"}}"#;

/// A task's session record as format 33 wrote it (theseus-nhew's build),
/// before the owner's flag on a task (`task.notify_done`, format 43,
/// theseus-j7cje): it reads unflagged, so its finish informs. By hand, in the
/// layout the build before theseus-j7cje writes.
pub(crate) const TASK_SESSION_BEFORE_NOTIFY: &str = r#"{"session_id":"ses_00000000000000000000000000000331","kind":"task","label":"task","created_at_unix_ms":1790000000331,"turns":1,"last_turn_id":"turn_00000000000000000000000000000333","usage":{"input_tokens":400,"output_tokens":60,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"execution_id":"exe_00000000000000000000000000000331","compilation_id":"cmp_00000000000000000000000000000332","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790000000339,"cost_usd":0.004,"tool_calls":0,"title":"Sound the outer channel","task":{"parent_session":"ses_00000000000000000000000000000330","parent_execution":"exe_00000000000000000000000000000330","by":"act_00000000000000000000000000000330","arrangement":"arr_00000000000000000000000000000331"}}"#;

/// A plan item as format 44 wrote it (theseus-a8n7's build; the layout is
/// unchanged since 19), before a task's long-term kind, date and review
/// (`kind`, `due_at_ms`, `review`, format 45, theseus-n5ui2). It reads as an
/// item with no date and no review, and writes back byte for byte. By hand, in
/// the layout the build before theseus-n5ui2 (persona-threads') writes.
pub(crate) const TASK_BEFORE_PROJECTS: &str = r#"{"id":"tsk_00000000000000000000000000000301","version":3,"title":"Chart the reef","objective":"chart the reef's north edge","acceptance":["every marker has a depth"],"state":"accepted","parent":"tsk_00000000000000000000000000000300","deps":[],"owner":"agent","origin":{"session":"ses_lighthouse","principal":"operator","by_model":true},"evidence":[],"claim":{"by":"exe_00000000000000000000000000000302","session":"ses_00000000000000000000000000000302","until_ms":1790000900000},"created_at_ms":1790000000301,"updated_at_ms":1790000000305}"#;

/// A session as format 44 wrote it, before the project it serves (`project`,
/// format 45, theseus-n5ui2). It reads with no project, and writes back byte
/// for byte. By hand, in the layout the build before theseus-n5ui2 writes.
pub(crate) const SESSION_BEFORE_PROJECT: &str = r#"{"session_id":"ses_00000000000000000000000000000311","kind":"conversation","label":null,"created_at_unix_ms":1790000000311,"turns":2,"last_turn_id":"turn_00000000000000000000000000000313","usage":{"input_tokens":700,"output_tokens":90,"cache_read_input_tokens":200,"cache_creation_input_tokens":50},"execution_id":"exe_00000000000000000000000000000311","compilation_id":"cmp_00000000000000000000000000000312","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790000000319,"cost_usd":0.0066,"tool_calls":0,"title":"Plan the harbour survey","dir":"/work/harbour"}"#;

/// A session as format 48 wrote it (unchanged since 45: its project),
/// before its data state (`data`, format 49, theseus-a1wvq.5). It reads with
/// none, the private view with no open run, and writes back byte for byte. By
/// hand, in the layout the build before theseus-a1wvq.5 writes.
pub(crate) const SESSION_BEFORE_DATA: &str = r#"{"session_id":"ses_00000000000000000000000000000481","kind":"conversation","label":null,"created_at_unix_ms":1790000000481,"turns":3,"last_turn_id":"turn_00000000000000000000000000000483","usage":{"input_tokens":900,"output_tokens":140,"cache_read_input_tokens":300,"cache_creation_input_tokens":60},"execution_id":"exe_00000000000000000000000000000481","compilation_id":"cmp_00000000000000000000000000000482","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790000000489,"cost_usd":0.0091,"tool_calls":1,"title":"Chart the breakwater","dir":"/work/harbour","project":"tsk_00000000000000000000000000000480"}"#;

/// A session as format 51 wrote it, holding the open view's data state
/// (`data`: opened, run, taint; format 49, theseus-a1wvq.5), before content
/// labels took the field over (format 52, theseus-9ymz0). It reads as a record
/// with no labels and nothing judged, and writes back its other fields byte
/// for byte; the three open-view fields are gone. **Generated, not typed**:
/// the build before theseus-9ymz0 serialised this exact record from its own
/// `DataState` (`probe-old-data`, ARM lane cfcut, 2026-10-26), and the text is
/// what it printed.
pub(crate) const SESSION_WITH_OPEN_VIEW_DATA: &str = r#"{"session_id":"ses_00000000000000000000000000000481","kind":"conversation","label":null,"created_at_unix_ms":1790000000481,"turns":3,"last_turn_id":"turn_00000000000000000000000000000483","usage":{"input_tokens":900,"output_tokens":140,"cache_read_input_tokens":300,"cache_creation_input_tokens":60},"execution_id":"exe_00000000000000000000000000000481","compilation_id":"cmp_00000000000000000000000000000482","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790000000489,"cost_usd":0.0091,"tool_calls":1,"title":"Chart the breakwater","dir":"/work/harbour","project":"tsk_00000000000000000000000000000480","data":{"opened":{"how":"task","at_ms":1790000000490},"run":{"from":"msg_00000000000000000000000000000484","through":"msg_00000000000000000000000000000486"},"taint":{"why":"sealed","at_ms":1790000000491}}}"#;

/// A session as format 52 wrote it, its `data` holding content labels (one
/// raised, one raised and removed, `judged_through`), before a label named its
/// judgment (`judgment`, format 53, theseus-gyjyo.3). It reads with no judgment
/// on the label, which then teaches nothing, and writes back byte for byte (an
/// empty judgment is not written). **Generated, not typed**: the build before
/// theseus-gyjyo.3 (209f9c52) serialised this record from its own
/// `ContentFlags` (an integration probe over `theseus-core`'s public types,
/// 2026-10-27), and the text is what it printed.
pub(crate) const SESSION_WITH_LABELS_AT_52: &str = r#"{"session_id":"ses_00000000000000000000000000000481","kind":"conversation","label":null,"created_at_unix_ms":1790000000481,"turns":3,"last_turn_id":"turn_00000000000000000000000000000483","usage":{"input_tokens":900,"output_tokens":140,"cache_read_input_tokens":300,"cache_creation_input_tokens":60},"execution_id":"exe_00000000000000000000000000000481","compilation_id":"cmp_00000000000000000000000000000482","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790000000489,"cost_usd":0.0091,"tool_calls":1,"title":"Chart the breakwater","dir":"/work/harbour","project":"tsk_00000000000000000000000000000480","data":{"labels":[{"flag":"secret","node":"msg_00000000000000000000000000000484","kind":"tool_result","source":"fs.read /srv/notes.txt","confidence":0.8,"band":"confirm","at_ms":1790000000490}],"removed":[{"flag":"confidential","by":"the CLI","via":"cli","at_ms":1790000000495}],"judged_through":"msg_00000000000000000000000000000486"}}"#;

/// A session as format 53 wrote it, its `data` holding the whole mark
/// (`judged_through` and the `unjudged` worklist) beside a label that names
/// its judgment, before the mark went (format 54, theseus-gyjyo.7). It reads
/// with the mark ignored (the labels and the removal are all it says), and
/// writes back without it. **Generated, not typed**: the build before
/// theseus-gyjyo.7 (f04c3d2a, with the probe `5aa9e972` carried) serialised this
/// record from its own `ContentFlags`, and the text is what it printed.
pub(crate) const SESSION_WITH_MARK_AT_53: &str = r#"{"session_id":"ses_00000000000000000000000000000481","kind":"conversation","label":null,"created_at_unix_ms":1790000000481,"turns":3,"last_turn_id":"turn_00000000000000000000000000000483","usage":{"input_tokens":900,"output_tokens":140,"cache_read_input_tokens":300,"cache_creation_input_tokens":60},"execution_id":"exe_00000000000000000000000000000481","compilation_id":"cmp_00000000000000000000000000000482","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790000000489,"cost_usd":0.0091,"tool_calls":1,"title":"Chart the breakwater","dir":"/work/harbour","project":"tsk_00000000000000000000000000000480","data":{"labels":[{"flag":"secret","node":"msg_00000000000000000000000000000484","kind":"tool_result","source":"fs.read /srv/notes.txt","confidence":0.8,"band":"confirm","at_ms":1790000000490,"judgment":"jdg_0000000000000000000000000000048a"}],"removed":[{"flag":"confidential","by":"the CLI","via":"cli","at_ms":1790000000495}],"judged_through":"msg_00000000000000000000000000000486","unjudged":["msg_00000000000000000000000000000485"]}}"#;

/// A kind row as format 45 wrote it, an `onto:kind:` META record: the topic
/// seed, before a kind's retirement, examples and mined sources
/// (`retired_ms`, `examples`, `mined_from`, format 46, theseus-e3mjc). It
/// reads as a live kind with none of them, names the operator and the import
/// alone, and writes back byte for byte. By hand, in the layout the build
/// before onto-core writes.
pub(crate) const KIND_BEFORE_ONTO_CORE: &str = r#"{"name":"topic","basis":"interpreted","assigned_by":["operator","import"],"per_session":3,"parent":"topic","precedence":40,"rule":"chain","description":"What a session is about, declared by the operator, or by the import from an imported session's labels. Topics nest.","version":1,"added_by":"seed"}"#;

/// An ontology category as format 49 wrote it, before a persona's card
/// (`persona`, format 50, theseus-utfnl): a person with a handle. It reads
/// with no card and writes back byte for byte. By hand, in the layout the
/// build before theseus-utfnl writes.
pub(crate) const CATEGORY_BEFORE_PERSONA: &str = r#"{"id":"person:ada-rowe","name":"Ada Rowe","description":"keeps the harbour log","added_by":"operator","handles":["email:ada@example.test"]}"#;

/// A card proposal as format 50 wrote it, an archive job's `card.proposal.` META record,
/// before the live pass's `ask`, `record`, `observations` and `posted_ms` (format 51,
/// theseus-6f8t0). It reads as an Add question and writes back byte for byte. By hand, in
/// the layout the build before theseus-6f8t0 writes.
pub(crate) const CARD_PROPOSAL_AT_50: &str = r#"{"id":"cdp_0000000000000501","kind":"preference","scope":{"kind":"everywhere"},"text":"Prefers tide tables in metres","question":"Add to your card: 'Prefers tide tables in metres' (from 2 sources)? [Keep] [Edit] [Not this]","options":["Keep","Edit","Not this"],"sources":[{"episode":"ep_0000501","node":"nod_0000501","kind":"feedback","at_ms":1790000000501}],"support":2,"created_ms":1790000000502,"expires_ms":1790001200502,"state":"kept","run":"crd_0000000000000501","answered_ms":1790000000503,"answered_by":"owner","standing_id":"std_0000000000000501"}"#;

#[test]
fn a_format_50_card_proposal_reads_as_an_add_and_writes_back_byte_for_byte() {
    let p: theseus_protocol::CardProposal = serde_json::from_str(CARD_PROPOSAL_AT_50).unwrap();
    assert_eq!(p.ask, theseus_protocol::CardAsk::Add);
    assert!(p.record.is_none() && p.observations.is_empty() && p.posted_ms.is_none());
    assert_eq!(p.state, theseus_protocol::ProposalState::Kept);
    assert_eq!(serde_json::to_string(&p).unwrap(), CARD_PROPOSAL_AT_50);
}

/// A routine's record as format 54 wrote it, the META key `routine:<session
/// id>`, before a start, an optional stop and what a run carries (`carry`,
/// `start_ms`, `stop`, `stopped_ms`; format 55, theseus-97b8c). It reads as a
/// routine that runs until it is paused, carrying the last run's report by
/// default, and writes back byte for byte: every field theseus-97b8c adds is
/// left out at its default. By hand, in the layout the build before
/// theseus-97b8c writes.
pub(crate) const ROUTINE_AT_54: &str = r#"{"session_id":"ses_00000000000000000000000000000540","execution_id":"exe_00000000000000000000000000000540","wake_id":"wak_00000000000000000000000000000541","title":"Morning digest","schedule":"weekdays 08:00","brief":"Summarize the overnight mail.","run_micros":500000,"month_micros":10000000,"catch_up":"auto","speak":"always","deliver_to":"dm","period_ms":86400000,"created_ms":1790000000540,"created_by":"owner","month":"2026-10","runs":22,"last_task":"exe_00000000000000000000000000000542"}"#;

#[test]
fn a_format_54_routine_reads_as_one_that_runs_until_it_is_paused() {
    let s: theseus_protocol::routines::RoutineSpec =
        serde_json::from_str(ROUTINE_AT_54).unwrap();
    assert!(s.stop.is_none() && s.stopped_ms.is_none() && s.start_ms.is_none());
    assert_eq!(s.carry, theseus_protocol::routines::Carry::LastReport);
    assert_eq!(s.runs, 22);
    assert_eq!(serde_json::to_string(&s).unwrap(), ROUTINE_AT_54);
}

const SAMPLES: &[Sample] = &[
    Sample {
        kind: kinds::META,
        layout: "a routine at format 54 (theseus-52tut's grant on its wake): before a start, an optional stop and what a run carries (55, theseus-97b8c)",
        bytes: ROUTINE_AT_54,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a session at format 53 (theseus-gyjyo.3's judgment on a label) holding the mark: before the mark went (54, theseus-gyjyo.7), which reads it ignored",
        bytes: SESSION_WITH_MARK_AT_53,
        kept: Kept::AllBut(&["/data/judged_through", "/data/unjudged"]),
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a session at format 52 (theseus-9ymz0's content labels): before a label named its judgment (53, theseus-gyjyo.3)",
        bytes: SESSION_WITH_LABELS_AT_52,
        // Its mark goes with theseus-gyjyo.7 (54); its labels and removal stay.
        kept: Kept::AllBut(&["/data/judged_through"]),
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a session at format 51 (theseus-a1wvq.5's data state: opened, run, taint): before content labels took `data` over (52, theseus-9ymz0), which reads it as no labels",
        bytes: SESSION_WITH_OPEN_VIEW_DATA,
        kept: Kept::AllBut(&["/data/opened", "/data/run", "/data/taint"]),
    },
    Sample {
        kind: kinds::META,
        layout: "a card proposal at format 50 (theseus-utfnl's; unchanged since 43): before its ask, record, observations and posted time (51, theseus-6f8t0)",
        bytes: CARD_PROPOSAL_AT_50,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::META,
        layout: "an ontology category at format 49 (theseus-a1wvq.5's): before a persona's card (50, theseus-utfnl)",
        bytes: CATEGORY_BEFORE_PERSONA,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a session at format 48 (theseus-q9b85's; unchanged since 45): before its data state (49, theseus-a1wvq.5)",
        bytes: SESSION_BEFORE_DATA,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::META,
        layout: "an ontology kind row at format 45 (theseus-n5ui2's): before its retirement, examples and mined sources (46, theseus-e3mjc)",
        bytes: KIND_BEFORE_ONTO_CORE,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::COMPILATION,
        layout: "a compilation at format 32 (theseus-2qzx's): before its manifest's loaded tool families (36, theseus-7m7w.2)",
        bytes: COMPILATION_BEFORE_LOADED,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::COMPILATION,
        layout: "a compilation at format 33 (theseus-nhew's; unchanged since 32): before the prefix library's header in its manifest (41, theseus-rr15)",
        bytes: COMPILATION_BEFORE_PREFIX,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a task's session at format 33 (theseus-nhew's): before the owner's flag on it (43, theseus-j7cje)",
        bytes: TASK_SESSION_BEFORE_NOTIFY,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::TASK,
        layout: "a plan item at format 44 (theseus-a8n7's; unchanged since 19): before a task's kind, date and review (45, theseus-n5ui2)",
        bytes: TASK_BEFORE_PROJECTS,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a session at format 44 (theseus-a8n7's; unchanged since 32): before the project it serves (45, theseus-n5ui2)",
        bytes: SESSION_BEFORE_PROJECT,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a session at format 29 (theseus-a7vi's; unchanged since 28): before a fork's origin (32, theseus-2qzx)",
        bytes: SESSION_BEFORE_FORK,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::COMPILATION,
        layout: "a compilation at format 29 (theseus-a7vi's): before its inherited run (32, theseus-2qzx)",
        bytes: COMPILATION_BEFORE_FORK,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a session at format 27 (theseus-wy7y's; unchanged since 25): before its directory (28, theseus-aab7)",
        bytes: SESSION_BEFORE_DIR,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::META,
        layout: "an ontology category at format 24 (theseus-anh3; unchanged through 25): before a person's handles and merge (26, theseus-wy7y)",
        bytes: CATEGORY_BEFORE_PEOPLE,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a session at format 24 (theseus-anh3; unchanged since 23): before its state (25, theseus-emqx)",
        bytes: SESSION_BEFORE_STATES,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::TASK,
        layout: "a plan item at format 16 (theseus-ext.10; unchanged through 18): before a task's claim (19, M7 39b)",
        bytes: TASK_BEFORE_CLAIM,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::TASK,
        layout: "a plan item at format 14 (39a; unchanged through 15): before the model's mark on its origin (16, theseus-ext.10)",
        bytes: TASK_BEFORE_MODEL_MARK,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "1, 460a35b's: before T1's hold on external text (2), its run of failures (3), the images its provider refused (4), a search's query (5), and the 1-hour cache writes (6)",
        bytes: r#"{"session_id":"ses_01a0f3f3c9b77474a7caa5d4925bd678","kind":"conversation","label":null,"created_at_unix_ms":1790799235511,"turns":1,"last_turn_id":"turn_01a0f3f3cd4774a9a2462c729fb82f1b","usage":{"input_tokens":100,"output_tokens":24,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"execution_id":"exe_01a0f3f3c9b77474a7caa5d505b05b5e","compilation_id":"cmp_01a0f3f3cd57766c8225231563419be3","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790799237649,"cost_usd":0.00044,"tool_calls":1,"title":"Start the background job."}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a conversation after a few turns, before its routed profile (format 15, M5 25e; unchanged through 14): its run of failures and the images its provider refused; by hand, in the layout the build before 25e (802f913) writes",
        bytes: r#"{"session_id":"ses_00000000000000000000000000000091","kind":"conversation","label":null,"created_at_unix_ms":1790000000091,"turns":3,"last_turn_id":"turn_00000000000000000000000000000093","usage":{"input_tokens":1200,"output_tokens":300,"cache_read_input_tokens":800,"cache_creation_input_tokens":400},"execution_id":"exe_00000000000000000000000000000091","compilation_id":"cmp_00000000000000000000000000000091","last_target":{"profile":"opus","provider":"anthropic","model":"claude-opus-5-5"},"last_active_ms":1790000000099,"cost_usd":0.0123,"tool_calls":2,"title":"Weigh the two log designs","failing":{"turns":1,"lasting":0,"class":"timeout","noticed":true,"parked":false,"since_ms":1790000000095},"not_shown":[{"digest":"a1b2c3d4e5f60718","why":"the image could not be read","at_ms":1790000000096}]}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a conversation routing moved, at format 15 (M5 25e; unchanged through 20): its routed profile and a switch the cache holds back, before the base it was moved from (21, theseus-0j2.17); by hand, in the layout the build before it (80ef1dea) writes",
        bytes: r#"{"session_id":"ses_00000000000000000000000000000095","kind":"conversation","label":null,"created_at_unix_ms":1790000000095,"turns":2,"last_turn_id":"turn_00000000000000000000000000000097","usage":{"input_tokens":900,"output_tokens":200,"cache_read_input_tokens":0,"cache_creation_input_tokens":600},"execution_id":"exe_00000000000000000000000000000095","compilation_id":"cmp_00000000000000000000000000000096","last_target":{"profile":"opus","provider":"anthropic","model":"claude-opus-5-5"},"last_active_ms":1790000000099,"cost_usd":0.0311,"tool_calls":0,"title":"Weigh two designs for a tide log","routed":{"profile":"opus","hold":{"mode":"routine_coding","profile":"glm53","turn":"turn_00000000000000000000000000000097"}}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a conversation routing moved, with its base, at format 21 (theseus-0j2.17; unchanged through 22): before an imported session's provenance, `imported` (23, theseus-0lrr.6); by hand, in the layout the build before it (c4f79e9f) writes",
        bytes: r#"{"session_id":"ses_00000000000000000000000000000221","kind":"conversation","label":null,"created_at_unix_ms":1790000000221,"turns":4,"last_turn_id":"turn_00000000000000000000000000000224","usage":{"input_tokens":2100,"output_tokens":420,"cache_read_input_tokens":1500,"cache_creation_input_tokens":300},"execution_id":"exe_00000000000000000000000000000221","compilation_id":"cmp_00000000000000000000000000000222","last_target":{"profile":"glm53","provider":"zai","model":"glm-5.3"},"last_active_ms":1790000000229,"cost_usd":0.0207,"tool_calls":1,"title":"Plan the reef survey","routed":{"profile":"glm53","from":"opus"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a task's session (DD7, W1): its origin and where it reports, before its arrangement's node (format 9, M5 27; unchanged through 8); the build before theseus-vug.2 (f1fccec) writes it back byte for byte",
        bytes: r#"{"session_id":"ses_00000000000000000000000000000081","kind":"task","label":"task","created_at_unix_ms":1790000000081,"turns":0,"last_turn_id":null,"usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"execution_id":"exe_00000000000000000000000000000081","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":0,"cost_usd":0.0,"tool_calls":0,"title":"Chart the harbour soundings","task":{"parent_session":"ses_lighthouse","parent_execution":"exe_lighthouse","by":"act_00000000000000000000000000000081","target":"discord:dm:42"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::SESSION,
        layout: "a task's session with its arrangement's node (format 9, M5 27; unchanged through 13), before a check's basis (14, M5 28a); the build before theseus-vug.3 (78d749c) writes it back byte for byte",
        bytes: r#"{"session_id":"ses_00000000000000000000000000000081","kind":"task","label":"task","created_at_unix_ms":1790000000081,"turns":0,"last_turn_id":null,"usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"execution_id":"exe_00000000000000000000000000000081","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":0,"cost_usd":0.0,"tool_calls":0,"title":"Chart the harbour soundings","task":{"parent_session":"ses_lighthouse","parent_execution":"exe_lighthouse","by":"act_00000000000000000000000000000081","target":"discord:dm:42","arrangement":"arr_00000000000000000000000000000083"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::EXECUTION,
        layout: "1, 460a35b's: before an execution's wakes, report wakes, and stop (2)",
        bytes: r#"{"id":"exe_01a0f3f3c9b77474a7caa5d505b05b5e","schema":2,"session_id":"ses_01a0f3f3c9b77474a7caa5d4925bd678","kind":"conversation","state":"cancelled","authority":{"principal":"operator","ceilings":{}},"budget":{"limit_micros":100000000,"spent_micros":440,"reserved_micros":0,"held_unknown_micros":0,"reservations":{},"resets":0},"outstanding":["act_01a0f3f3cd97711e8a0b5fbbdc8b2bbc"],"queued_results":[],"turns":1,"interrupted":0,"resume_pending":false,"cancel":"requested","ended_reason":"cancelled by sock#4","created_at_ms":1790799235511,"updated_at_ms":1790799237745}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::EXECUTION,
        layout: "2 (DD8, W1): an execution holding a one-shot wake and its target, before 37a's repeat and occurrence (format 5)",
        bytes: r#"{"id":"exe_00000000000000000000000000000071","schema":2,"session_id":"ses_lantern","kind":"conversation","state":"waiting","authority":{"principal":"operator","ceilings":{}},"budget":{"limit_micros":100000,"spent_micros":0,"reserved_micros":0,"held_unknown_micros":0,"reservations":{},"resets":0,"pinned":true},"wake":{"on":"input"},"outstanding":[],"queued_results":[],"wakes":[{"id":"wak_00000000000000000000000000000072","due_at_ms":1790000060000,"note":"check the tide tables","set_at_ms":1790000000000,"by":"act_00000000000000000000000000000072","target":"discord:dm:7"}],"turns":1,"interrupted":0,"resume_pending":false,"created_at_ms":1790000000000,"updated_at_ms":1790000000000}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::EXECUTION,
        layout: "an execution holding a check-in and a weekly series as format 29 wrote them, before a wake's purpose, run and pause (format 30, theseus-8ux6x): each reads as a check-in that takes a turn, and writes back byte for byte",
        bytes: r#"{"id":"exe_00000000000000000000000000000073","schema":2,"session_id":"ses_lantern","kind":"conversation","state":"waiting","authority":{"principal":"operator","ceilings":{}},"budget":{"limit_micros":100000,"spent_micros":0,"reserved_micros":0,"held_unknown_micros":0,"reservations":{},"resets":0,"pinned":true},"wake":{"on":"input"},"outstanding":[],"queued_results":[],"wakes":[{"id":"wak_00000000000000000000000000000074","due_at_ms":1790000060000,"note":"check the tide tables","set_at_ms":1790000000000,"by":"act_00000000000000000000000000000074","target":"discord:dm:7"},{"id":"wak_00000000000000000000000000000075","due_at_ms":1790086400000,"note":"water the ferns","set_at_ms":1790000000000,"by":"act_00000000000000000000000000000075","repeat":{"every":{"n":1,"unit":"d"},"first_ms":1790086400000,"days":["mon","fri"],"until_ms":1792000000000},"occurrence":3}],"turns":1,"interrupted":0,"resume_pending":false,"created_at_ms":1790000000000,"updated_at_ms":1790000000000}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::ACTION,
        layout: "1, 460a35b's: a job's action, before a cancel's verdict (3) and a parent (4)",
        bytes: r#"{"correlation_id":"act_01a0f3f3cd97711e8a0b5fbbdc8b2bbc","schema":2,"execution_id":"exe_01a0f3f3c9b77474a7caa5d505b05b5e","session_id":"ses_01a0f3f3c9b77474a7caa5d4925bd678","tool":"proc.run","args_digest":"4533d5d67e5454984ee9e9dda4071126ee57335316dadaf778c88aace4ad066a","resource":"/tmp/theseus-f4a/fixgen/projects","retry_class":{"class":"non_repeatable"},"state":"planned","deadline_at_ms":1790802866503,"planned_at_ms":1790799236503,"reserved_micros":0,"completions_seen":0}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::ACTION,
        layout: "2: a job a cancel verified gone, before 18a's verdict (3); the build before b77ffe9 (08b595d) writes it back byte for byte",
        bytes: r#"{"correlation_id":"act_00000000000000000000000000000041","schema":2,"execution_id":"exe_lighthouse","session_id":"ses_lighthouse","tool":"proc.run","args_digest":"7ad721861f8d37f2a13e31ce6588122b125276a670f610806e09c6516d851cd6","retry_class":{"class":"non_repeatable"},"state":"cancelled","deadline_at_ms":1790000060041,"planned_at_ms":1790000000041,"authorized_at_ms":1790000000041,"dispatched_at_ms":1790000000041,"settled_at_ms":1790000001041,"cancel":"termination_verified","reserved_micros":0,"completions_seen":0}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::ACTION,
        layout: "3 (M4 18a): a cancel's verdict, before a parent (4); the kernel frames golden's at 18a",
        bytes: r#"{"correlation_id":"act_00000000000000000000000000000043","schema":2,"execution_id":"exe_lighthouse","session_id":"ses_lighthouse","tool":"proc.run","args_digest":"7ad721861f8d37f2a13e31ce6588122b125276a670f610806e09c6516d851cd6","retry_class":{"class":"non_repeatable"},"state":"cancelled","deadline_at_ms":1790000060043,"planned_at_ms":1790000000043,"authorized_at_ms":1790000000043,"dispatched_at_ms":1790000000043,"settled_at_ms":1790000001043,"cancel":"termination_verified","verdict":{"verified_by":"tree","killed":2,"survivors":0,"ms":0},"reserved_micros":0,"resolution":"stopped by the operator","completions_seen":0}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::ACTION,
        layout: "a hand a cancel reached at format 7 (step 40 part 1; unchanged through 10): unsupported, as an in-process call, before a hand's own stop and ECS's verdict (11, theseus-mgw.11)",
        bytes: r#"{"correlation_id":"act_00000000000000000000000000000081","schema":2,"execution_id":"exe_lighthouse","session_id":"ses_lighthouse","tool":"aws.hand","args_digest":"5d0b8e2a7c4f1936d8a2b5e0c3f6a9d2b5e8c1f4a7d0b3e6c9f2a5d8b1e4c7f0","resource":"aws.hands.group.act_00000000000000000000000000000080","retry_class":{"class":"non_repeatable"},"state":"cancelled","deadline_at_ms":1790000900081,"planned_at_ms":1790000000081,"authorized_at_ms":1790000000081,"dispatched_at_ms":1790000000081,"settled_at_ms":1790000001081,"cancel":"unsupported","verdict":{"verified_by":"none","ms":0,"why":"it runs in process to its end, within its deadline"},"reserved_micros":0,"resolution":"the execution was cancelled by the CLI","completions_seen":0}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::OUTBOX,
        layout: "1: a notice its channel took, before 18a's verdict (2); the build before b77ffe9 (08b595d) writes it back byte for byte",
        bytes: r#"{"correlation_id":"out_00000000000000000000000000000044","schema":2,"execution_id":"","session_id":"ses_lighthouse","tool":"outbox","args_digest":"9f2c7a1e4b8d3f6a0c5e9b2d7f1a4c8e3b6d9f0a2c5e8b1d4f7a0c3e6b9d2f5a","proposal":{"tool":"outbox","args":{"kind":"notice","text":"the harbour opens at six"},"resource":"discord:dm:42","policy_context":null},"resource":"discord:dm:42","retry_class":{"class":"idempotent_with_key","key":"discord.nonce"},"state":"succeeded","deadline_at_ms":0,"planned_at_ms":1790000000044,"authorized_at_ms":1790000000044,"dispatched_at_ms":1790000000045,"settled_at_ms":1790000000144,"reserved_micros":0,"completions_seen":1,"detail":{"messages":["m_44"]}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::OUTBOX,
        layout: "2: a card its channel took, before a parent (3)",
        bytes: r#"{"correlation_id":"out_00000000000000000000000000000045","schema":2,"execution_id":"exe_lighthouse","session_id":"ses_lighthouse","tool":"outbox","args_digest":"4b1e9c7a2d5f8b0e3a6c9d2f5b8e1a4c7d0f3b6e9a2c5d8f1b4e7a0c3d6f9b2e","proposal":{"tool":"outbox","args":{"kind":"card","node":"tcl_00000000000000000000000000000045","question":"act_00000000000000000000000000000045"},"resource":"discord:dm:42","policy_context":null},"resource":"discord:dm:42","retry_class":{"class":"idempotent_with_key","key":"discord.nonce"},"state":"succeeded","deadline_at_ms":0,"planned_at_ms":1790000000045,"authorized_at_ms":1790000000045,"dispatched_at_ms":1790000000046,"settled_at_ms":1790000000146,"reserved_micros":0,"completions_seen":1,"detail":{"messages":["m_45"]}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::COMPLETION,
        layout: "1, 460a35b's: a provider call's completion, before 2",
        bytes: r#"{"correlation_id":"act_01a0f3f3cd6d775481958e681b008a38","outcome":"succeeded","result_ref":"msg_01a0f3f3cd867192886d8ac6d440b3ce","started_at_ms":1790799236484,"finished_at_ms":1790799236486,"producer":"provider:anthropic","cost_micros":200,"detail":{"message_id":"msg_bench","served_model":"claude-sonnet-5-5"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "1, 460a35b's: an assistant message that calls a tool (unchanged through 26), before an answer's effort (27, theseus-o719)",
        bytes: r#"{"id":"msg_01a0f3f3cd867192886d8ac6d440b3ce","schema":1,"session_id":"ses_01a0f3f3c9b77474a7caa5d4925bd678","turn_id":"turn_01a0f3f3cd4774a9a2462c729fb82f1b","loop_index":0,"origin":"agent","author":null,"created_at_ms":1790799236486,"body":{"kind":"assistant_message","blocks":[{"id":"toolu_bench_job","input":{"argv":["sleep","300"],"timeout_secs":3600},"name":"proc_run","type":"tool_use"}],"model":"claude-sonnet-5-5","provider":"anthropic","stop_reason":"tool_use","usage":{"input_tokens":40,"output_tokens":12,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"cost_usd":0.0002,"catalog_version":"2026-09-29.1+config:12","request_id":null,"correlation_id":"act_01a0f3f3cd6d775481958e681b008a38","compilation_id":"cmp_01a0f3f3cd57766c8225231563419be3","request_digest":"0f287003f798a76f51e504047d99d54c3612deb2b4866aeef9068aa8ef47ba67"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "1, 460a35b's: a tool call and its gate record, before a plan's class and AWS call (3)",
        bytes: r#"{"id":"tcl_01a0f3f3cd97711e8a0b5fbcdb8c77c0","schema":1,"session_id":"ses_01a0f3f3c9b77474a7caa5d4925bd678","turn_id":"turn_01a0f3f3cd4774a9a2462c729fb82f1b","loop_index":0,"origin":"harness","author":null,"created_at_ms":1790799236503,"body":{"kind":"tool_call","tool_use_id":"toolu_bench_job","tool":"proc.run","wire_name":"proc_run","input":{"argv":["sleep","300"],"timeout_secs":3600},"assistant_node":"msg_01a0f3f3cd867192886d8ac6d440b3ce","correlation_id":"act_01a0f3f3cd97711e8a0b5fbbdc8b2bbc","gate":{"decision":{"posture":"open","reason":"proc.run — open (enforcement = open)"},"plan":{"argv":["sleep","300"],"resources":[{"access":"exec","path":"/tmp/theseus-f4a/fixgen/projects"}],"summary":"run `sleep 300` in /tmp/theseus-f4a/fixgen/projects"},"proposal":{"args":{"argv":["sleep","300"],"timeout_secs":3600},"policy_context":{"cwd":"/tmp/theseus-f4a/fixgen/projects","roots":["/tmp/theseus-f4a/fixgen/projects"]},"resource":"/tmp/theseus-f4a/fixgen/projects","tool":"proc.run"},"result":{"gate":"allow"},"validated":true}}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "1, 460a35b's: a background job's tool result",
        bytes: r#"{"id":"trs_01a0f3f3d1bb76449493cbd85e5ca250","schema":1,"session_id":"ses_01a0f3f3c9b77474a7caa5d4925bd678","turn_id":"turn_01a0f3f3cd4774a9a2462c729fb82f1b","loop_index":0,"origin":"tool","author":null,"created_at_ms":1790799237563,"body":{"kind":"tool_result","tool_use_id":"toolu_bench_job","tool":"proc.run","status":"background","is_error":false,"content":"Still running as background job act_01a0f3f3cd97711e8a0b5fbbdc8b2bbc after 1 seconds (timeout 3600 seconds). Its result will arrive in a later message; you can keep working or tell the operator you are waiting.","correlation_id":"act_01a0f3f3cd97711e8a0b5fbbdc8b2bbc","bytes_total":210,"truncated":false,"full_ref":null,"duration_ms":null,"late":false,"meta":{"pid":390608}}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "2: a tool call's plan with no class and no AWS call (theseus-ppsd, 3); the layout by hand, as e85efb0's build writes it back byte for byte",
        bytes: r#"{"id":"tcl_00000000000000000000000000000021","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t2","loop_index":0,"origin":"harness","author":null,"created_at_ms":1790000000021,"body":{"kind":"tool_call","tool_use_id":"tu_21","tool":"fs.read","wire_name":"fs_read","input":{"path":"/w/log/tides.md"},"assistant_node":"asm_00000000000000000000000000000021","correlation_id":"act_t21","gate":{"decision":{"posture":"open","reason":"fs.read — open (policy.tools)"},"plan":{"resources":[{"access":"read","path":"/w/log/tides.md"}],"summary":"read /w/log/tides.md"},"proposal":{"args":{"path":"/w/log/tides.md"},"policy_context":{"cwd":"/w","roots":["/w"]},"resource":"/w/log/tides.md","tool":"fs.read"},"result":{"gate":"allow"},"validated":true}}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "3 (theseus-ppsd): a plan's class, and no class in the decision (theseus-7ve.1, 4); the build before 4fc7ddc (b503b2c) writes it back byte for byte",
        bytes: r#"{"id":"tcl_00000000000000000000000000000031","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t2","loop_index":0,"origin":"harness","author":null,"created_at_ms":1790000000021,"body":{"kind":"tool_call","tool_use_id":"tu_21","tool":"proc.run","wire_name":"fs_read","input":{"path":"/w/log/tides.md"},"assistant_node":"asm_00000000000000000000000000000021","correlation_id":"act_t21","gate":{"decision":{"posture":"open","reason":"fs.read — open (policy.tools)"},"plan":{"class":"run","resources":[{"access":"read","path":"/w/log/tides.md"}],"summary":"read /w/log/tides.md"},"proposal":{"args":{"path":"/w/log/tides.md"},"policy_context":{"cwd":"/w","roots":["/w"]},"resource":"/w/log/tides.md","tool":"fs.read"},"result":{"gate":"allow"},"validated":true}}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "4 (17b): an L1 call's class in its decision, and no label (19a, 5); the build before 42a27af (1d33622) writes it back byte for byte",
        bytes: r#"{"id":"tcl_00000000000000000000000000000041","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t2","loop_index":0,"origin":"harness","author":null,"created_at_ms":1790000000021,"body":{"kind":"tool_call","tool_use_id":"tu_21","tool":"proc.run","wire_name":"fs_read","input":{"path":"/w/log/tides.md"},"assistant_node":"asm_00000000000000000000000000000021","correlation_id":"act_t21","gate":{"decision":{"class":"l1","posture":"open","reason":"fs.read — open (policy.tools)"},"plan":{"class":"run","resources":[{"access":"read","path":"/w/log/tides.md"}],"summary":"read /w/log/tides.md"},"proposal":{"args":{"path":"/w/log/tides.md"},"policy_context":{"cwd":"/w","roots":["/w"]},"resource":"/w/log/tides.md","tool":"fs.read"},"result":{"gate":"allow"},"validated":true}}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "5 (19a): an owner-only file's result with its label, which the place rule dropped (theseus-nbsh, 7); the build before 751780d (79f2d1d) writes it back byte for byte",
        bytes: r#"{"id":"trs_00000000000000000000000000000051","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t5","loop_index":0,"origin":"tool","author":null,"created_at_ms":1790000000051,"body":{"kind":"tool_result","tool_use_id":"tu_51","tool":"fs.read","status":"ok","is_error":false,"content":"the vault code is 4417","correlation_id":"act_t51","bytes_total":22,"truncated":false,"full_ref":null,"duration_ms":3,"late":false,"meta":{}},"label":{"integrity":"trusted","readers":"owner"}}"#,
        kept: Kept::AllBut(&["/label"]),
    },
    Sample {
        kind: kinds::NODE,
        layout: "6 (19c): a graduated node, its label naming a place's readers and the operator's warrant, which the place rule dropped (7)",
        bytes: r#"{"id":"msg_00000000000000000000000000000061","schema":1,"session_id":"ses_lighthouse","turn_id":null,"loop_index":null,"origin":"operator","author":"cli","created_at_ms":1790000000061,"body":{"kind":"user_message","text":"the vault code is 4417"},"label":{"integrity":"trusted","readers":{"place":"discord:7"},"warrant":{"graduated_from":"trs_00000000000000000000000000000051","who":"cli","how":"cli","why":"the code is for the whole lab","at_ms":1790000000062}}}"#,
        kept: Kept::AllBut(&["/label"]),
    },
    Sample {
        kind: kinds::NODE,
        layout: "7 (DD7, 12a): a task's brief, relayed from its parent, before the arrangement node that follows it (format 9, M5 27; unchanged through 8); the build before theseus-vug.2 (f1fccec) writes it back byte for byte",
        bytes: r#"{"id":"msg_00000000000000000000000000000082","schema":1,"session_id":"ses_00000000000000000000000000000081","turn_id":null,"loop_index":null,"origin":"agent","author":"session:ses_lighthouse","created_at_ms":1790000000081,"body":{"kind":"user_message","text":"[Task 000081: a background task started by session …thouse.]\n\nChart the harbour soundings."}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "7 (theseus-8kk.1; unchanged through 9): a user message of origin `operator`, as every input was written before an MCP prompt's `mcp` origin (10, theseus-ext.4); the builds before it write it back byte for byte",
        bytes: r#"{"id":"msg_00000000000000000000000000000071","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t7","loop_index":null,"origin":"operator","author":"cli","created_at_ms":1790000000071,"body":{"kind":"user_message","text":"say hello to Ada"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "a user message with its files at format 16 (theseus-9g2; unchanged since): a text file cut, an image by its digest, and a file not read, before a file kept whole (17, theseus-c9l6); the build before it (7ad8a8bd) wrote it",
        bytes: r#"{"id":"msg_00000000000000000000000000000093","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t9","loop_index":null,"origin":"operator","author":"discord:zeroaltitude","created_at_ms":1790000000093,"body":{"kind":"user_message","text":"what do these say?","attachments":[{"name":"harbour-notes.txt","media_type":"text/plain","size":48,"content":{"kind":"text","text":"Moorings are free after six; the pilot boards at the outer mark.","cut":true}},{"name":"chart.png","media_type":"image/png","size":1033,"content":{"kind":"image","digest":"6b1e2f0c9a4d7e3b5c8f1a2d4e6b9c0f3a5d7e9b1c4f6a8d0e2b5c7f9a1d3e6b","width":1280,"height":720}},{"name":"soundings.pdf","media_type":"application/pdf","size":1200000,"content":{"kind":"not_read","reason":"only text files and images are read"}}]}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "a task's arrangement node (format 9, M5 27; unchanged through 13): a trusted piece, a superseded one, and the fidelity ack, before a check's claim (14, M5 28a); the build before theseus-vug.3 (78d749c) writes it back byte for byte",
        bytes: r#"{"id":"arr_00000000000000000000000000000083","schema":1,"session_id":"ses_00000000000000000000000000000081","turn_id":null,"loop_index":null,"origin":"agent","author":"session:ses_lighthouse","created_at_ms":1790000000081,"body":{"kind":"arrangement","pieces":[{"role":"objective","node":"msg_00000000000000000000000000000071","session_id":"ses_lighthouse","origin":"operator","author":"cli","at_ms":1790000000071,"text":"Chart the harbour soundings before the spring tide.","first_line":"Chart the harbour soundings before the spring tide.","trusted":true},{"role":"context","node":"msg_00000000000000000000000000000072","session_id":"ses_lighthouse","origin":"agent","author":null,"at_ms":1790000000072,"first_line":"The old chart is from 1998.","superseded_by":0}],"fidelity_ack":true}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "a `proc.run` call and its gate record at format 17 (unchanged from 3 through 19): one argv, before a batch's `steps` on its plan (20, theseus-7gir.3); by hand, in the layout the build before it (4d11e797) writes",
        bytes: r#"{"id":"tcl_00000000000000000000000000000097","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t9","loop_index":0,"origin":"harness","author":null,"created_at_ms":1790000000097,"body":{"kind":"tool_call","tool_use_id":"tu_97","tool":"proc.run","wire_name":"proc_run","input":{"argv":["make","charts"],"cwd":"/w/harbour"},"assistant_node":"asm_00000000000000000000000000000097","correlation_id":"act_t97","gate":{"decision":{"notify":{"kind":"notify","rule":"proc.run — notify (enforcement = notify)","setting":"enforcement = notify"},"posture":"notify","reason":"proc.run — notify (enforcement = notify)"},"plan":{"argv":["make","charts"],"resources":[{"access":"exec","path":"/w/harbour"}],"summary":"run `make charts` in /w/harbour"},"proposal":{"args":{"argv":["make","charts"],"cwd":"/w/harbour"},"policy_context":{"cwd":"/w","roots":["/w"]},"resource":"/w/harbour","tool":"proc.run"},"result":{"gate":"allow"},"validated":true}}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::COMPILATION,
        layout: "1, 460a35b's: a new session's compilation, before its cache layout (3)",
        bytes: r#"{"id":"cmp_01a0f3f3cd57766c8225231563419be3","schema":1,"session_id":"ses_01a0f3f3c9b77474a7caa5d4925bd678","created_at_ms":1790799236439,"trigger":"new_session","strategy":"transcript","as_of":49,"includes":["msg_01a0f3f3cd4f72d28cedea2e393aa42d"],"derived_from":null,"manifest":{"compiler_version":1,"renderer_version":1,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"27aaef021b7a8f84","tools_digest":"57a95134045b2d98","tools":["fs_edit","fs_glob","fs_grep","fs_list","fs_patch","fs_read","fs_write","git_diff","git_log","proc_run","text_diff"],"catalog_version":"2026-09-29.1+config:12","context_window":1000000,"strip_thinking":false}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::COMPILATION,
        layout: "3 (theseus-ev1): its cache layout and a context file, before the audience (4); the build before 42a27af (1d33622) writes it back byte for byte; and the place rule's (5, unchanged at 6), which dropped the audience, before the ontology's memberships and guidance (7, theseus-8kk.1)",
        bytes: r#"{"id":"cmp_00000000000000000000000000000051","schema":1,"session_id":"ses_lighthouse","created_at_ms":1790000000051,"trigger":"new_session","strategy":"transcript","as_of":17,"includes":["msg_00000000000000000000000000000051"],"derived_from":null,"manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read"],"catalog_version":"2026-10-01","context_window":1000000,"strip_thinking":false,"context_files":[{"path":"/w/NOTES.md","digest":"a1b2c3d4e5f60718","bytes":12}],"cache":{"caches":true,"min_tokens":2048,"blocks":[{"block":"header","prefix_bytes":9000,"marked":true}]}}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::COMPILATION,
        layout: "4 (19a): the audience, the readers, the integrity, and a withheld node, which the place rule dropped (5), with a context file's readers",
        bytes: r#"{"id":"cmp_00000000000000000000000000000052","schema":1,"session_id":"ses_lighthouse","created_at_ms":1790000000052,"trigger":"audience","strategy":"transcript","as_of":18,"includes":["msg_00000000000000000000000000000051"],"derived_from":null,"manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read"],"catalog_version":"2026-10-01","context_window":1000000,"strip_thinking":false,"context_files":[{"path":"/w/NOTES.md","bytes":0,"withheld":"owner-only"},{"path":"/w/open/README.md","digest":"0f1e2d3c4b5a6978","bytes":9,"readers":"public"}],"cache":{"caches":true,"min_tokens":2048,"blocks":[{"block":"header","prefix_bytes":9000,"marked":true}]},"audience":{"kind":"place","place":"discord:314159265358979323","name":"lab","viewers":2,"digest":"0f1e2d3c4b5a6978"},"readers":{"place":"discord:314159265358979323"},"integrity":{"latched":false,"untrusted":0},"withheld":[{"node_id":"trs_00000000000000000000000000000052","reason":"owner-only"}]}}"#,
        kept: Kept::AllBut(&["/manifest/audience", "/manifest/readers", "/manifest/integrity", "/manifest/withheld", "/manifest/context_files/1/readers"]),
    },
    Sample {
        kind: kinds::COMPILATION,
        layout: "7 (21b, theseus-8kk.1): the ontology's memberships and guidance, before M6's budget (8, theseus-6fn.2); the build before 30b (175318e) writes it back byte for byte",
        bytes: r#"{"id":"cmp_00000000000000000000000000000071","schema":1,"session_id":"ses_lighthouse","created_at_ms":1790000000071,"trigger":"new_session","strategy":"transcript","as_of":19,"includes":["msg_00000000000000000000000000000071"],"derived_from":null,"manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read"],"catalog_version":"2026-10-01","context_window":1000000,"strip_thinking":false,"cache":{"caches":true,"min_tokens":2048,"blocks":[{"block":"header","prefix_bytes":9000,"marked":true}]},"memberships":[{"kind":"channel","category":"channel:314159265358979323","origin":"transport","as_of_ms":1790000000071},{"kind":"topic","category":"topic:tides","origin":"operator","as_of_ms":1790000000070}],"guidance":[{"category":"topic:tides","version":2,"digest":"a1b2c3d4e5f60718"}]}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::COMPILATION,
        layout: "8 (30b, theseus-6fn.2): a ring's compilation with M6's budget, before an assembled prefix's recall_id (13, theseus-6fn.4); by hand, in the layout the build before 30c (760553f) writes",
        bytes: r#"{"id":"cmp_00000000000000000000000000000081","schema":1,"session_id":"ses_lighthouse","created_at_ms":1790000000081,"trigger":"overflow","strategy":"ring","as_of":31,"includes":["msg_00000000000000000000000000000083","asm_00000000000000000000000000000084"],"derived_from":"cmp_00000000000000000000000000000071","manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read"],"catalog_version":"2026-10-01","context_window":40000,"strip_thinking":true,"cache":{"caches":true,"min_tokens":2048,"blocks":[{"block":"header","prefix_bytes":9000,"marked":true}]}},"budget":{"limit_tokens":33904,"used_tokens":12010,"dropped":[{"range":{"first":"msg_00000000000000000000000000000081","last":"asm_00000000000000000000000000000082","nodes":2},"reason":"overflow","tokens":9400,"tier":"ring"},{"node_id":"msg_00000000000000000000000000000085","reason":"budget","tokens":410,"tier":"recall"}]}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::COMPILATION,
        layout: "13 (30c, theseus-6fn.4): a compaction's compilation with its budget and an assembled prefix's recall_id (unchanged through 21), before its situation (22, M6 35a); by hand, in the layout the build before 35a (d5a4b80) writes",
        bytes: r#"{"id":"cmp_00000000000000000000000000000131","schema":1,"session_id":"ses_lighthouse","created_at_ms":1790000000131,"trigger":"overflow","strategy":"compaction","as_of":57,"includes":["sum_00000000000000000000000000000133","msg_00000000000000000000000000000132"],"derived_from":"cmp_00000000000000000000000000000081","manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read"],"catalog_version":"2026-10-01","context_window":40000,"strip_thinking":true,"cache":{"caches":true,"min_tokens":2048,"blocks":[{"block":"header","prefix_bytes":9000,"marked":true}]}},"budget":{"limit_tokens":33904,"used_tokens":8012,"dropped":[{"range":{"first":"msg_00000000000000000000000000000081","last":"msg_00000000000000000000000000000131","nodes":14},"reason":"overflow","tokens":21000,"tier":"compaction"}]},"recall_id":"rcn_00000000000000000000000000000134"}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::COMPILATION,
        layout: "22 (M6 35a) through 28: a compilation with its situation, before its manifest's standing list (29, theseus-a7vi); by hand, in the layout the build before it (d50ad5e) writes",
        bytes: r#"{"id":"cmp_00000000000000000000000000000271","schema":1,"session_id":"ses_lighthouse","created_at_ms":1790000000271,"trigger":"new_session","strategy":"transcript","as_of":61,"includes":["msg_00000000000000000000000000000271"],"derived_from":null,"manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read"],"catalog_version":"2026-10-01","context_window":1000000,"strip_thinking":false,"context_files":[{"path":"/w/NOTES.md","digest":"a1b2c3d4e5f60718","bytes":12}],"cache":{"caches":true,"min_tokens":2048,"blocks":[{"block":"header","prefix_bytes":9000,"marked":true},{"block":"context","prefix_bytes":9400,"marked":true}]}},"situation":{"kind":"conversation_start"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::LEDGER,
        layout: "a row of a kind no build writes now (theseus-w5op): 18d's secret.requested",
        bytes: r#"{"at_unix_ms":1790000000047,"kind":"secret.requested","session_id":"ses_lighthouse","data":{"command":"sh","correlation_id":"act_00000000000000000000000000000047","job":"act_00000000000000000000000000000043","kind":"secret","outcome":"granted","posture":"notify","secret":"github_token","setting":"proc.run ran at notify","why":null}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::LEDGER,
        layout: "a row of a kind no build writes now (theseus-w5op): 18d's secret.declined",
        bytes: r#"{"at_unix_ms":1790000000048,"kind":"secret.declined","session_id":"ses_lighthouse","data":{"by":"the CLI","correlation_id":"act_00000000000000000000000000000048","job":"act_00000000000000000000000000000043","secret":"github_token","why":"not today"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::LEDGER,
        layout: "a row of a kind no build writes now (the place rule, theseus-nbsh): 19a's or 19c's label.audience",
        bytes: r#"{"at_unix_ms":1790000000071,"kind":"label.audience","data":{"name":"lab","place":"discord:314159265358979323","viewers":2,"why":null}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::LEDGER,
        layout: "a row of a kind no build writes now (the place rule, theseus-nbsh): 19a's or 19c's label.withheld",
        bytes: r#"{"at_unix_ms":1790000000072,"kind":"label.withheld","session_id":"ses_lighthouse","data":{"audience":{"digest":"0f1e2d3c4b5a6978","kind":"place","name":"lab","place":"discord:314159265358979323","viewers":2},"compilation_id":"cmp_00000000000000000000000000000052","reasons":{"owner-only":{"context_files":0,"nodes":1}},"withheld":1}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::LEDGER,
        layout: "a row of a kind no build writes now (the place rule, theseus-nbsh): 19a's or 19c's label.graduated",
        bytes: r#"{"at_unix_ms":1790000000073,"kind":"label.graduated","session_id":"ses_lighthouse","data":{"node_id":"msg_00000000000000000000000000000061","readers":{"place":"discord:7"},"source":"trs_00000000000000000000000000000051","why":"the code is for the whole lab"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::LEDGER,
        layout: "a row of a kind no build writes now (the place rule, theseus-nbsh): 19a's or 19c's label.held_post",
        bytes: r#"{"at_unix_ms":1790000000074,"kind":"label.held_post","session_id":"ses_lighthouse","data":{"post":"out_00000000000000000000000000000074","question":"act_00000000000000000000000000000074","target":"discord:channel:314159265358979323"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::LEDGER,
        layout: "a row as format 29 wrote it, before the ledger's hash chain (format 31, theseus-bmkxk): no `chain` field, which reads as an unchained row",
        bytes: r#"{"at_unix_ms":1790000000301,"kind":"turn.started","session_id":"ses_lighthouse","turn_id":"turn_00000000000000000000000000000301","data":{"loops":1}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::LEDGER,
        layout: "a row as format 31 writes it: the writer's `chain` field (`<seq>.<digest>`) spliced in last; a LedgerRow reads it and leaves it out, and the chain's own reader recomputes it",
        bytes: r#"{"at_unix_ms":1790000000302,"kind":"turn.ended","session_id":"ses_lighthouse","data":{"loops":1},"chain":"7.3fa1c09e5d2b7a64b8c1e0f93a7d2c55e1f4b6a8093d7c2e1b5a4f6d8c0e9b37"}"#,
        kept: Kept::AllBut(&["/chain"]),
    },
    Sample {
        kind: kinds::LEDGER,
        layout: "a row of a kind no build writes now (the place rule, theseus-nbsh): 19a's or 19c's label.held_post_answered",
        bytes: r#"{"at_unix_ms":1790000000075,"kind":"label.held_post_answered","session_id":"ses_lighthouse","data":{"approve":false,"by":"the CLI","question":"act_00000000000000000000000000000074"}}"#,
        kept: Kept::All,
    },
    Sample {
        kind: kinds::NODE,
        layout: "a user message as format 33 wrote it (theseus-nhew's build; the layout is unchanged since 7), before the surface it was written from (`surface`, format 38, theseus-slvl.3): it reads with none and renders with no tag; by hand, in the layout the build before it (e354320) writes",
        bytes: r#"{"id":"msg_00000000000000000000000000000341","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t34","loop_index":null,"origin":"operator","author":"sock#3","created_at_ms":1790000000341,"body":{"kind":"user_message","text":"quick one: is the tide in?"}}"#,
        kept: Kept::All,
    },
];

/// A payload of `kind`, decoded as this build's reader decodes it, and
/// encoded again.
fn reread(kind: RecordKind, bytes: &[u8]) -> anyhow::Result<String> {
    fn again<T: serde::de::DeserializeOwned + serde::Serialize>(
        b: &[u8],
    ) -> anyhow::Result<String> {
        Ok(serde_json::to_string(&serde_json::from_slice::<T>(b)?)?)
    }
    match kind {
        kinds::SESSION => again::<SessionRecord>(bytes),
        kinds::LEDGER => again::<LedgerRow>(bytes),
        kinds::EXECUTION => Ok(serde_json::to_string(&Execution::from_stored(
            bytes,
            100_000_000,
        )?)?),
        kinds::ACTION | kinds::OUTBOX => again::<Action>(bytes),
        kinds::COMPLETION => again::<Completion>(bytes),
        kinds::NODE => again::<Node>(bytes),
        kinds::COMPILATION => again::<Compilation>(bytes),
        // New at format 12 (39a); its layout before 16 is a sample above.
        kinds::TASK => again::<crate::task_graph::TaskRecord>(bytes),
        // META holds many records; its samples here are the ontology's
        // kind rows (`onto:kind:`, which have a basis) and categories
        // (`onto:cat:`).
        kinds::META if String::from_utf8_lossy(bytes).contains(r#""basis":"#) => {
            again::<theseus_ontology::Kind>(bytes)
        }
        // A card proposal (`card.proposal.`) has a question the others lack.
        kinds::META if String::from_utf8_lossy(bytes).contains(r#""question":"#) => {
            again::<theseus_protocol::CardProposal>(bytes)
        }
        // A routine's record (`routine:<session>`) names the wake that holds
        // its schedule, which no other META sample has.
        kinds::META if String::from_utf8_lossy(bytes).contains(r#""wake_id":"#) => {
            again::<theseus_protocol::routines::RoutineSpec>(bytes)
        }
        kinds::META => again::<theseus_ontology::Category>(bytes),
        k => anyhow::bail!("kind {k} has no reader here"),
    }
}

/// Take the field at JSON pointer `at` out of `v`: whether it was there.
fn take(v: &mut Value, at: &str) -> bool {
    let (parent, field) = at.rsplit_once('/').unwrap_or(("", at));
    match v.pointer_mut(parent) {
        Some(Value::Object(m)) => m.remove(field).is_some(),
        _ => false,
    }
}

#[test]
fn every_old_layout_on_disk_still_reads() {
    for s in SAMPLES {
        let name = format!("{} {}", kinds::name(s.kind), s.layout);
        let again = reread(s.kind, s.bytes.as_bytes())
            .unwrap_or_else(|e| panic!("{name}: it does not decode: {e:#}"));
        match s.kept {
            Kept::All => assert_eq!(again, s.bytes, "{name}: a field was lost or changed"),
            Kept::AllBut(gone) => {
                let mut want: Value = serde_json::from_str(s.bytes).unwrap();
                for at in gone {
                    assert!(take(&mut want, at), "{name}: the sample has no {at}");
                }
                let got: Value = serde_json::from_str(&again).unwrap();
                assert_eq!(
                    got, want,
                    "{name}: a field but {gone:?} was lost or changed"
                );
            }
        }
        assert_eq!(
            reread(s.kind, again.as_bytes()).unwrap(),
            again,
            "{name}: a round trip lost something"
        );
    }
}

/// A store of format 28, which holds a compilation as the build before the
/// owner's standing records (theseus-a7vi) wrote it and no standing record:
/// the build opens it, reads the compilation, finds no records, takes one,
/// and moves the manifest to this build's format, which an older build then
/// refuses.
#[test]
fn a_format_28_store_opens_with_no_standing_records_and_takes_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let sample = SAMPLES
        .iter()
        .find(|s| s.layout.starts_with("22 (M6 35a) through 28"))
        .unwrap();
    {
        let store = crate::store::Store::open(&path).unwrap();
        let c: Compilation = serde_json::from_str(sample.bytes).unwrap();
        store
            .append(&[theseus_store::NewRecord::bytes(
                kinds::COMPILATION,
                Some(&c.id),
                sample.bytes.as_bytes().to_vec(),
            )
            .scoped(&c.session_id)])
            .unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 28, "engine": "redb"}"#).unwrap();

    let store = crate::store::Store::open(&path).unwrap();
    let read = store
        .get_compilation("cmp_00000000000000000000000000000271")
        .unwrap()
        .unwrap();
    assert!(read.manifest.standing.is_empty());
    let board = crate::standing::Board::default();
    assert!(board.all(&store).unwrap().is_empty());
    let r = theseus_protocol::StandingRecord {
        id: "std_00000028".into(),
        kind: theseus_protocol::StandingKind::Order,
        text: "never push to main".into(),
        scope: theseus_protocol::StandingScope::Everywhere,
        since_ms: 1_790_000_000_000,
        until_ms: None,
        source: "the CLI".into(),
        ends: None,
    };
    board
        .write(&store, |_| Ok(vec![r.clone()]), |_, _| Ok(vec![]))
        .unwrap();
    store
        .append(&[theseus_store::NewRecord::json(kinds::LEDGER, None, &"x").unwrap()])
        .unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    // This build's format: the move is made by the first write of any build.
    assert_eq!(moved["format"], 55);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    assert_eq!(
        *crate::standing::Board::default().all(&again).unwrap(),
        vec![r]
    );
}

/// A store of format 29, whose ledger holds rows with no chain (theseus-bmkxk):
/// the build opens it and reads them, its first ledger write starts the chain
/// with a row that counts and pins them, and the manifest moves to 45. The
/// chain then verifies, with the old rows pinned by count.
#[test]
fn a_format_29_store_starts_its_chain_over_the_rows_it_holds() {
    use theseus_store::wal::{Wal, WalConfig};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    std::fs::create_dir_all(&path).unwrap();
    {
        let wal = Wal::open(&path.join("wal"), WalConfig::default()).unwrap();
        for sample in SAMPLES
            .iter()
            .filter(|s| s.layout.starts_with("a row as format 29"))
        {
            wal.append(&[theseus_store::NewRecord::bytes(
                kinds::LEDGER,
                None,
                sample.bytes.as_bytes().to_vec(),
            )])
            .unwrap();
        }
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 29, "engine": "redb"}"#).unwrap();

    let store = crate::store::Store::open(&path).unwrap();
    let before = theseus_store::chain_walk::verify(store.inner().as_ref(), None, None).unwrap();
    assert!(
        before.broken.is_none() && before.head.is_none(),
        "{before:?}"
    );
    assert_eq!(before.unchained, 1);
    store
        .append_ledger(&LedgerRow::new(
            theseus_protocol::LedgerKind::TurnStarted,
            Some("ses_lighthouse"),
            None,
            Value::Null,
        ))
        .unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
    let after = theseus_store::chain_walk::verify(store.inner().as_ref(), None, None).unwrap();
    assert!(after.broken.is_none(), "{after:?}");
    assert_eq!((after.rows, after.unchained), (2, 1));
}

/// A store of format 29, which holds a session and a compilation as the
/// build before forks (theseus-2qzx) wrote them: the build opens it, reads
/// both with no origin and no inherited run, and moves the manifest to 49,
/// which an older build then refuses. A fork written then reads back.
#[test]
fn a_format_29_store_opens_with_no_forks_and_takes_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    {
        let store = crate::store::Store::open(&path).unwrap();
        let s: SessionRecord = serde_json::from_str(SESSION_BEFORE_FORK).unwrap();
        let c: Compilation = serde_json::from_str(COMPILATION_BEFORE_FORK).unwrap();
        store
            .append(&[
                theseus_store::NewRecord::bytes(
                    kinds::SESSION,
                    Some(&s.session_id),
                    SESSION_BEFORE_FORK.as_bytes().to_vec(),
                ),
                theseus_store::NewRecord::bytes(
                    kinds::COMPILATION,
                    Some(&c.id),
                    COMPILATION_BEFORE_FORK.as_bytes().to_vec(),
                )
                .scoped(&c.session_id),
            ])
            .unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 29, "engine": "redb"}"#).unwrap();

    let store = crate::store::Store::open(&path).unwrap();
    let sid = "ses_00000000000000000000000000000291";
    assert_eq!(crate::fork::origin_of(&store, sid).unwrap(), None);
    let read = store
        .get_compilation("cmp_00000000000000000000000000000292")
        .unwrap()
        .unwrap();
    assert!(read.inherits.is_none());
    let mut fork = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    fork.forked_from = Some(Box::new(theseus_protocol::ForkOrigin {
        session: sid.into(),
        node: "msg_00000000000000000000000000000291".into(),
        position: 3,
        compilation: None,
        at_ms: 1_790_000_000_300,
    }));
    store.put_session(&fork.session_id, &fork).unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    let origin = crate::fork::origin_of(&again, &fork.session_id)
        .unwrap()
        .unwrap();
    assert_eq!((origin.session.as_str(), origin.position), (sid, 3));
}

/// A thread's record as format 33 wrote it (theseus-nhew, the first layout): a
/// literal, so a later change to `ThreadInfo` must still read it and write it
/// back byte for byte. The session and the last activity are not stored.
pub(crate) const THREAD_AT_33: &str = r#"{"slug":"tide-gauges","title":"Tide gauge calibration","goal":"Recalibrate gauge 4","topics":["topic:tides"],"working_set":{"roots":["~/gauges"],"repos":["gauge-log"],"refs":["PR#12"]},"card":["gauge 4 drifts 2 mm a week"],"persona":"surveyor","state":"paused","sealed":{"by":"the CLI","at_ms":1790000000330,"reason":"private survey"},"seal_pending":"keep it private","surfaces":["cli"],"parent":"harbour","merged_into":"harbour-2","split_from":"harbour-0","last_active_ms":1790000000331,"created_at_ms":1790000000329}"#;

#[test]
fn a_format_33_thread_record_reads_and_writes_back_byte_for_byte() {
    let t: theseus_protocol::threads::ThreadInfo = serde_json::from_str(THREAD_AT_33).unwrap();
    assert_eq!(t.slug, "tide-gauges");
    assert_eq!(t.state, theseus_protocol::threads::ThreadState::Paused);
    assert_eq!(t.working_set.refs, ["PR#12"]);
    assert_eq!(serde_json::to_string(&t).unwrap(), THREAD_AT_33);
    // A record with only what a thread must have reads, with the rest default.
    let min: theseus_protocol::threads::ThreadInfo =
        serde_json::from_str(r#"{"slug":"a","title":"A"}"#).unwrap();
    assert_eq!(min.state, theseus_protocol::threads::ThreadState::Active);
    assert!(min.card.is_empty() && min.session_id.is_none());
}

/// A thread's record as format 47 wrote it, with a seal and a `split_from` and no `links` (theseus-98bz.11
/// added them at 48): it reads with none and writes back byte for byte, an empty list being left out.
pub(crate) const THREAD_AT_47: &str = r#"{"slug":"harbour-ledger","title":"Harbour ledger","goal":"Close the quarter","topics":["topic:ledger"],"working_set":{"repos":["harbour-books"]},"card":["March is reconciled"],"state":"active","sealed":{"by":"the CLI","at_ms":1790000000440,"reason":"before disclosure"},"surfaces":["cli"],"split_from":"harbour","last_active_ms":1790000000441,"created_at_ms":1790000000439}"#;

#[test]
fn a_format_47_thread_record_reads_and_writes_back_byte_for_byte() {
    let t: theseus_protocol::threads::ThreadInfo = serde_json::from_str(THREAD_AT_47).unwrap();
    assert_eq!(t.slug, "harbour-ledger");
    assert!(t.links.is_empty() && t.sealed.is_some());
    assert_eq!(t.split_from.as_deref(), Some("harbour"));
    assert_eq!(serde_json::to_string(&t).unwrap(), THREAD_AT_47);
    // A thread with links keeps them, after `split_from`.
    let linked = THREAD_AT_47.replace(
        r#""split_from":"harbour","#,
        r#""split_from":"harbour","links":["harbour","tide-gauges"],"#,
    );
    let t: theseus_protocol::threads::ThreadInfo = serde_json::from_str(&linked).unwrap();
    assert_eq!(t.links, ["harbour", "tide-gauges"]);
    assert_eq!(serde_json::to_string(&t).unwrap(), linked);
}

/// A store of format 32, which holds a session and no thread: the build opens
/// it, reads an empty board, takes a thread and moves the manifest to 49,
/// which an older build then refuses.
#[test]
fn a_format_32_store_opens_with_no_threads_and_takes_them() {
    use theseus_protocol::threads::{ThreadInfo, ThreadState};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    {
        let store = crate::store::Store::open(&path).unwrap();
        let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        store.put_session(&rec.session_id, &rec).unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 32, "engine": "redb"}"#).unwrap();

    let store = crate::store::Store::open(&path).unwrap();
    let threads = crate::threads::Threads::default();
    assert!(threads.all(&store).unwrap().is_empty());
    let t: ThreadInfo = serde_json::from_str(THREAD_AT_33).unwrap();
    threads
        .write(&store, |_| Ok(vec![t.clone()]), |_, _| Ok(vec![]))
        .unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    let board = crate::threads::Threads::default().all(&again).unwrap();
    let got = board.get("tide-gauges").unwrap();
    assert_eq!(got.state, ThreadState::Paused);
    assert_eq!(got.card, t.card);
}

/// A repeating wake as format 33 wrote it, before a series could be a cron
/// expression or a month span (theseus-pvm5c, store format 34): a literal, so a
/// later change to `PendingWake` and `Repeat` must still read it, with no cron,
/// and write it back byte for byte.
pub(crate) const WAKE_AT_33: &str = r#"{"id":"wak_01a0f3f3cd97711e8a0b5fbbdc8b2bbc","due_at_ms":1790838000000,"note":"water the ferns","set_at_ms":1790800000000,"by":"act_01a0f3f3cd97711e8a0b5fbbdc8b2bbc","repeat":{"every":{"n":1,"unit":"d"},"first_ms":1790838000000,"days":["mon","fri"],"until_ms":1793000000000},"occurrence":3}"#;

#[test]
fn a_format_33_repeating_wake_reads_with_no_cron_and_writes_back_byte_for_byte() {
    let w: theseus_kernel::PendingWake = serde_json::from_str(WAKE_AT_33).unwrap();
    let r = w.repeat.as_ref().unwrap();
    assert_eq!(r.cron, None);
    assert_eq!(r.every.to_string(), "1d");
    assert_eq!(r.days.len(), 2);
    assert_eq!(w.purpose, theseus_kernel::Purpose::Check);
    assert_eq!(serde_json::to_string(&w).unwrap(), WAKE_AT_33);
    // A series with a cron expression (format 34) round-trips too, and the
    // old build's reader would have ignored the field: that is why it is a bump.
    let mut cron = w.clone();
    cron.repeat.as_mut().unwrap().cron = Some("0 8 * * 1-5".into());
    let text = serde_json::to_string(&cron).unwrap();
    assert!(text.contains(r#""cron":"0 8 * * 1-5""#));
    let again: theseus_kernel::PendingWake = serde_json::from_str(&text).unwrap();
    assert_eq!(again, cron);
}

/// A failed task's execution record as format 33 wrote it (theseus-7zgsm,
/// store format 35): no `dismissed`, no `retry_of`. A literal, so a later
/// change must still read it, say it is no dismissal, and write it back byte
/// for byte.
pub(crate) const EXECUTION_AT_33: &str = r#"{"id":"exe_01a13bdf111d745f97fb57142347f1ff","schema":2,"session_id":"ses_01a13bdf111d745f97fb57142347f1ff","kind":"task","state":"failed","authority":{"principal":"zeroaltitude","ceilings":{"shell":"l1","tools":"fs,text"}},"budget":{"limit_micros":30000,"spent_micros":0,"reserved_micros":0,"held_unknown_micros":0,"reservations":{},"resets":0,"pinned":true},"outstanding":[],"queued_results":[],"parent":"exe_01a13bdf111873f3b1daec95a22683fb","reports_to":"discord:dm:1","wake_parent":true,"turns":1,"interrupted":0,"resume_pending":false,"ended_reason":"the provider refused the request (400)","created_at_ms":1000000,"updated_at_ms":1000000}"#;

#[test]
fn a_format_33_execution_reads_as_undismissed_and_writes_back_byte_for_byte() {
    let e: theseus_kernel::Execution = serde_json::from_str(EXECUTION_AT_33).unwrap();
    assert_eq!(e.state, theseus_kernel::ExecState::Failed);
    assert_eq!(e.dismissal(), None);
    assert!(e.retry_of.is_none());
    assert_eq!(serde_json::to_string(&e).unwrap(), EXECUTION_AT_33);
    // Dismissed, it carries the owner's word and reads it back.
    let mut d = e;
    d.dismissed = Some(theseus_kernel::Dismissal { at_ms: 5, turns: 1 });
    d.retry_of = Some("exe_old".into());
    let again: theseus_kernel::Execution =
        serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(again, d);
    assert_eq!(again.dismissal(), Some(5));
}

/// A store of format 33 holding a failed task: the build opens it, reads it as
/// not dismissed, takes the owner's dismissal and moves the manifest to 34.
#[test]
fn a_format_33_store_opens_with_its_failure_undismissed_and_takes_the_dismissal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let id = "exe_01a13bdf111d745f97fb57142347f1ff";
    {
        let store = crate::store::Store::open(&path).unwrap();
        store
            .append(&[theseus_store::NewRecord::bytes(
                kinds::EXECUTION,
                Some(id),
                EXECUTION_AT_33.as_bytes().to_vec(),
            )])
            .unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 33, "engine": "redb"}"#).unwrap();
    let store = crate::store::Store::open(&path).unwrap();
    let kernel = theseus_kernel::Kernel::new(
        store.shared(),
        std::sync::Arc::new(theseus_kernel::RealClock),
        theseus_kernel::KernelConfig::default(),
    );
    kernel.startup(None, &theseus_kernel::NoEvidence).unwrap();
    assert_eq!(kernel.execution(id).unwrap().unwrap().dismissal(), None);
    kernel.dismiss_execution(id).unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
    drop(kernel);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    use theseus_store::Store as _;
    let rec = again
        .inner()
        .latest_by_key(kinds::EXECUTION, id)
        .unwrap()
        .unwrap();
    let e: theseus_kernel::Execution = serde_json::from_slice(&rec.payload).unwrap();
    assert!(e.dismissal().is_some());
}

/// A compilation as format 37 wrote it (own-benchmark, theseus-pw1q.12): each
/// system block of its cache layout names its text by SHA-256 (`digest`), the
/// text in the blobs. A literal, so a later change to the layout must read it
/// and write it back byte for byte; the earlier layouts above, which carry no
/// digest, still read and write back without one.
pub(crate) const COMPILATION_AT_34: &str = r#"{"id":"cmp_00000000000000000000000000000341","schema":1,"session_id":"ses_lighthouse","created_at_ms":1790000000341,"trigger":"new_session","strategy":"transcript","as_of":81,"includes":["msg_00000000000000000000000000000341"],"derived_from":null,"manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read"],"catalog_version":"2026-10-01","context_window":1000000,"strip_thinking":false,"cache":{"caches":true,"min_tokens":2048,"blocks":[{"block":"header","prefix_bytes":9000,"marked":true,"digest":"cad307321e83979f3900d77b4fa45d4f722eab609eaedc9528beec55daa1982a"}]}}}"#;

#[test]
fn a_format_34_compilation_reads_and_writes_back_byte_for_byte() {
    let c: crate::compiler::Compilation = serde_json::from_str(COMPILATION_AT_34).unwrap();
    let b = &c.manifest.cache.as_ref().unwrap().blocks[0];
    assert_eq!(b.block, "header");
    assert_eq!(b.digest.as_deref().map(str::len), Some(64));
    assert_eq!(serde_json::to_string(&c).unwrap(), COMPILATION_AT_34);
    // The same compilation before 37 has no digest, and writes none.
    let old = COMPILATION_AT_34.replace(
        r#","digest":"cad307321e83979f3900d77b4fa45d4f722eab609eaedc9528beec55daa1982a""#,
        "",
    );
    let c: crate::compiler::Compilation = serde_json::from_str(&old).unwrap();
    assert!(c.manifest.cache.as_ref().unwrap().blocks[0]
        .digest
        .is_none());
    assert_eq!(serde_json::to_string(&c).unwrap(), old);
}

/// A store of format 33 opens, and its first write moves the manifest to 49,
/// which an older build then refuses.
#[test]
fn a_format_33_store_opens_and_takes_a_stamped_compilation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    {
        let store = crate::store::Store::open(&path).unwrap();
        let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        store.put_session(&rec.session_id, &rec).unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 33, "engine": "redb"}"#).unwrap();
    let store = crate::store::Store::open(&path).unwrap();
    let c: crate::compiler::Compilation = serde_json::from_str(COMPILATION_AT_34).unwrap();
    store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::COMPILATION,
            Some(&c.id),
            &c,
        )
        .unwrap()
        .scoped(&c.session_id)])
        .unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    assert_eq!(again.get_compilation(&c.id).unwrap().unwrap(), c);
}

/// A store of format 46 (the build before theseus-q9b85) opens with every node it held, and its
/// first write of a moved range's record (`Body::Moved`, format 47) moves the manifest, which an
/// older build then refuses. The record reads back whole, and its stub says what it is.
#[test]
fn a_format_46_store_opens_with_its_nodes_and_takes_a_moved_range() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    let sid = rec.session_id.clone();
    let first = crate::node::Node::user(&sid, Some("turn_1"), "owner", "the old tide table");
    {
        let store = crate::store::Store::open(&path).unwrap();
        store.put_session(&sid, &rec).unwrap();
        store.append(&[first.record().unwrap()]).unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 46, "engine": "redb"}"#).unwrap();
    let store = crate::store::Store::open(&path).unwrap();
    assert_eq!(
        store.transcript(&sid).unwrap().len(),
        1,
        "nothing is dropped on opening"
    );
    let note = crate::node::Node::harness_body(
        "mov",
        &sid,
        crate::moved::AUTHOR,
        crate::node::Body::Moved {
            split: "spl_1".into(),
            undoes: None,
            thread: "gauges".into(),
            to_session: "ses_gauges".into(),
            nodes: vec![first.id],
            first: 1,
            last: 1,
            gist: "the old tide table".into(),
            quiet: false,
        },
    );
    store.append(&[note.record().unwrap()]).unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    let nodes = again.transcript(&sid).unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[1].1.kind, crate::stub::Kind::Moved);
    assert_eq!(nodes[1].1.node(), std::sync::Arc::new(note));
    assert_eq!(crate::moved::read(&nodes).hidden.len(), 1);
}

/// A message that records its surface (format 34) writes the code beside its
/// author and reads it back; a node written before it has none, and keeps
/// its bytes whatever the build (the sample above).
#[test]
fn a_user_message_records_its_surface_and_an_old_one_has_none() {
    let old = r#"{"id":"msg_00000000000000000000000000000341","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t34","loop_index":null,"origin":"operator","author":"sock#3","created_at_ms":1790000000341,"body":{"kind":"user_message","text":"quick one: is the tide in?"}}"#;
    let n: Node = serde_json::from_str(old).unwrap();
    assert_eq!(n.surface, None);
    let mut n = n;
    n.surface = Some("terminal:tui".into());
    let v = serde_json::to_value(&n).unwrap();
    assert_eq!(v["surface"], "terminal:tui");
    let back: Node = serde_json::from_value(v).unwrap();
    assert_eq!(back.surface.as_deref(), Some("terminal:tui"));
}

/// A standing record as format 33 wrote it (theseus-a7vi's scopes: session,
/// place, everywhere): a literal, so the surface scope added at 34
/// (theseus-slvl.4) must leave it reading and writing back byte for byte.
pub(crate) const STANDING_AT_33: &str = r#"{"id":"std_00000331","kind":"preference","text":"short answers","scope":{"kind":"place","of":"discord:dm:7"},"since_ms":1790000000331,"source":"the CLI"}"#;

/// A surface scope is one more variant: `{"kind":"surface","of":"terminal"}`.
#[test]
fn a_format_33_standing_record_reads_and_a_surface_scope_writes_back() {
    use theseus_protocol::{StandingRecord, StandingScope};
    let old: StandingRecord = serde_json::from_str(STANDING_AT_33).unwrap();
    assert_eq!(old.scope, StandingScope::Place("discord:dm:7".into()));
    assert_eq!(serde_json::to_string(&old).unwrap(), STANDING_AT_33);
    let new = StandingRecord {
        scope: StandingScope::Surface("terminal".into()),
        ..old
    };
    let bytes = serde_json::to_string(&new).unwrap();
    assert!(
        bytes.contains(r#""scope":{"kind":"surface","of":"terminal"}"#),
        "{bytes}"
    );
    assert_eq!(serde_json::from_str::<StandingRecord>(&bytes).unwrap(), new);
}

/// A store of format 33 that holds a standing record: the build opens it,
/// reads the record, takes a surface-scoped one, and moves the manifest to 49,
/// which an older build then refuses.
#[test]
fn a_format_33_store_with_standing_records_takes_a_surface_scope() {
    use theseus_protocol::{StandingRecord, StandingScope};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    {
        let store = crate::store::Store::open(&path).unwrap();
        let old: StandingRecord = serde_json::from_str(STANDING_AT_33).unwrap();
        crate::standing::Board::default()
            .write(&store, |_| Ok(vec![old.clone()]), |_, _| Ok(vec![]))
            .unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 33, "engine": "redb"}"#).unwrap();

    let store = crate::store::Store::open(&path).unwrap();
    let board = crate::standing::Board::default();
    assert_eq!(board.all(&store).unwrap().len(), 1);
    let mut next: StandingRecord = serde_json::from_str(STANDING_AT_33).unwrap();
    next.id = "std_00000332".into();
    next.scope = StandingScope::Surface("terminal".into());
    board
        .write(&store, |_| Ok(vec![next.clone()]), |_, _| Ok(vec![]))
        .unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    let all = crate::standing::Board::default().all(&again).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all
        .iter()
        .any(|r| r.scope == StandingScope::Surface("terminal".into())));
}

/// A pre-image as format 40 wrote it (theseus-23rk, the record kind's first
/// layout): a literal, so a later change to `PreImage` must still read it and
/// write it back byte for byte.
pub(crate) const PRE_IMAGE_AT_40: &str = r#"{"session_id":"ses_harbour","call_id":"toolu_edit1","tool":"fs.edit","path":"/work/tides.py","before":{"state":"kept","digest":"6b1e2f0c9a4d7e3b5c8f1a2d4e6b9c0f3a5d7e9b1c4f6a8d0e2b5c7f9a1d3e6b","bytes":812},"after":"9f2c1d7e0a5b3c8d6e4f1a2b7c9d0e3f5a8b6c4d2e1f0a9b8c7d6e5f4a3b2c1d","added":1,"removed":1,"at_ms":1790000000340}"#;

#[test]
fn a_format_40_pre_image_reads_and_writes_back_byte_for_byte() {
    let p: crate::rewind::PreImage = serde_json::from_str(PRE_IMAGE_AT_40).unwrap();
    assert_eq!(p.tool, "fs.edit");
    assert!(matches!(
        p.before,
        crate::rewind::Before::Kept { bytes: 812, .. }
    ));
    assert_eq!(serde_json::to_string(&p).unwrap(), PRE_IMAGE_AT_40);
    // The other states of a file before an edit round-trip too.
    for before in [
        crate::rewind::Before::Absent,
        crate::rewind::Before::TooBig { bytes: 2_000_000 },
        crate::rewind::Before::Unread {
            why: "not a regular file".into(),
        },
    ] {
        let q = crate::rewind::PreImage {
            before,
            after: None,
            ..p.clone()
        };
        let again: crate::rewind::PreImage =
            serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
        assert_eq!(again, q);
    }
}

/// A store of format 39, which holds a session and no pre-image: the build
/// opens it, takes a pre-image, and moves the manifest to this build's format, which an older
/// build then refuses. The record reads back through its scope.
#[test]
fn a_format_39_store_opens_and_takes_a_pre_image() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    {
        let store = crate::store::Store::open(&path).unwrap();
        let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        store.put_session(&rec.session_id, &rec).unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 39, "engine": "redb"}"#).unwrap();

    let store = crate::store::Store::open(&path).unwrap();
    assert!(store
        .scope_after("rewind:ses_harbour", 0)
        .unwrap()
        .is_empty());
    let p: crate::rewind::PreImage = serde_json::from_str(PRE_IMAGE_AT_40).unwrap();
    store.append(&[p.record(0).unwrap()]).unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    let rows = crate::rewind::rows(&again, "ses_harbour").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, p);
}

/// A prefix header as format 41 writes it (theseus-rr15, the first layout):
/// a literal, so a later change to `prefix::Header` must still read it and
/// write it back byte for byte.
pub(crate) const PREFIX_HEADER_AT_41: &str = r##"{"digest":"a3f1","layer":"project","class":"private","lower":"9c0e","sections":[{"name":"files","digest":"5d2b","bytes":24},{"name":"# Guidance (topic tides)","digest":"77e0","bytes":52}],"text":"The harbour log. wave wave\n\n# Guidance (topic tides)\n\nQuote every level in metres.","sketch":[12,-3,0,127],"created_at_ms":1790000000342}"##;

#[test]
fn a_format_33_compilation_reads_with_no_header_and_a_header_writes_back_byte_for_byte() {
    let c: Compilation = serde_json::from_str(COMPILATION_BEFORE_PREFIX).unwrap();
    assert!(c.manifest.prefix.is_none());
    assert_eq!(
        serde_json::to_string(&c).unwrap(),
        COMPILATION_BEFORE_PREFIX
    );
    let h: crate::prefix::Header = serde_json::from_str(PREFIX_HEADER_AT_41).unwrap();
    assert_eq!((h.layer.as_str(), h.class.as_str()), ("project", "private"));
    assert_eq!(h.sections.len(), 2);
    assert_eq!(serde_json::to_string(&h).unwrap(), PREFIX_HEADER_AT_41);
    let mut with = c;
    with.manifest.prefix = Some(crate::prefix::PrefixUse {
        header: "a3f1".into(),
        fresh: "b7c2".into(),
    });
    let back: Compilation = serde_json::from_str(&serde_json::to_string(&with).unwrap()).unwrap();
    assert_eq!(back, with);
}

/// A store of format 33, which holds a session and no header: the build
/// opens it, loads an empty library, takes a header in a frame and moves the
/// manifest to 34, which an older build then refuses; a reopened store holds
/// the header and its hit log.
#[test]
fn a_format_33_store_opens_with_no_headers_and_takes_one() {
    use theseus_store::NewRecord;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    {
        let store = crate::store::Store::open(&path).unwrap();
        let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        store.put_session(&rec.session_id, &rec).unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 33, "engine": "redb"}"#).unwrap();

    let store = crate::store::Store::open(&path).unwrap();
    let lib = crate::prefix::Library::default();
    lib.loaded(&store).unwrap();
    assert_eq!(lib.stats(0).headers, 0);
    let h: crate::prefix::Header = serde_json::from_str(PREFIX_HEADER_AT_41).unwrap();
    let hits = crate::prefix::Hits {
        reuses: 2,
        last_read_ms: 1_790_000_000_400,
    };
    store
        .append(&[
            NewRecord::json(
                kinds::META,
                Some(&format!("{}{}", crate::prefix::HEADER, h.digest)),
                &h,
            )
            .unwrap(),
            NewRecord::json(
                kinds::META,
                Some(&format!("{}{}", crate::prefix::HITS, h.digest)),
                &hits,
            )
            .unwrap(),
        ])
        .unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    let lib = crate::prefix::Library::default();
    lib.loaded(&again).unwrap();
    assert_eq!(lib.stats(0).headers, 1);
    let plan = lib
        .replay(&crate::prefix::PrefixUse {
            header: h.digest.clone(),
            fresh: h.digest.clone(),
        })
        .unwrap();
    assert_eq!(&*plan.header, h.text.as_str());
}

/// An extension's manifest as format 33 wrote it (43a's layout, unchanged
/// since), before an ask for hands (`capabilities.hands`, format 42,
/// theseus-pw1q.14): it reads with no hands, so its load gets no channel.
/// By hand, in the layout the build before theseus-pw1q.14 (0baeed6) writes.
pub(crate) const MANIFEST_BEFORE_HANDS: &str = r#"{"name":"tidewc","digest":"3f2a1c00000000000000000000000000000000000000000000000000009e4b7d","description":"Counts the words of a tide log.","command":["python3","server.py"],"source":"/work/tidewc","frozen":"/state/extensions/tidewc/3f2a1c00000000000000000000000000000000000000000000000000009e4b7d","files":2,"bytes":180,"tools":[{"name":"count","description":"Counts words.","input_schema":{"type":"object"}}],"tests":[{"tool":"count","arguments":{"text":"two words"},"expect":{"contains":"2"},"passed":true,"got":"2"}],"capabilities":{"network":[],"scratch":true},"proposed_by":{"session_id":"ses_00000000000000000000000000000341","execution_id":"exe_00000000000000000000000000000341","correlation_id":"act_00000000000000000000000000000341","principal":"operator"},"proposed_at_ms":1790000000340,"question":"act_00000000000000000000000000000342","state":"acked","answered_by":"the CLI","answered_at_ms":1790000000341}"#;

/// The loaded extensions' record (META `extensions`) as format 33 wrote it
/// (43b's layout), before `capabilities.hands`: no hands, no channel. By
/// hand, in the build before theseus-pw1q.14's layout.
pub(crate) const LOADED_BEFORE_HANDS: &str = r#"{"loaded":{"tidewc":{"name":"tidewc","digest":"3f2a1c00000000000000000000000000000000000000000000000000009e4b7d","description":"Counts the words of a tide log.","command":["python3","server.py"],"frozen":"/state/extensions/tidewc/3f2a1c00000000000000000000000000000000000000000000000000009e4b7d","source":"/work/tidewc","tools":["count"],"capabilities":{"network":[],"scratch":true},"acked_by":"the CLI","acked_via":"cli","acked_at_ms":1790000000341,"question":"act_00000000000000000000000000000342","proposed_by":{"session_id":"ses_00000000000000000000000000000341","execution_id":"exe_00000000000000000000000000000341","correlation_id":"act_00000000000000000000000000000341","principal":"operator"}}}}"#;

/// A hands group's record (META `aws.hands.group.<group>`) as format 33
/// wrote it (step 40's layout, unchanged since part 2), before its
/// `extension` (format 42, theseus-pw1q.14): a model's own call, which
/// writes no `extend.hands.settled` row. Its bytes are a group the build
/// writes with no extension, which leaves the field out, trimmed to one hand.
pub(crate) const GROUP_BEFORE_EXTENSION: &str = r#"{"v":1,"group":"act_00000000000000000000000000000331","execution_id":"exe_00000000000000000000000000000331","session_id":"ses_00000000000000000000000000000331","account":"111122223333","region":"us-west-2","deployment":"theseus-example","backend":"lambda","request":{"argv":["sh","-c","transform {input}"],"inputs":[["in/file-000.txt","in/file-001.txt","in/file-002.txt","in/file-003.txt","in/file-004.txt","in/file-005.txt","in/file-006.txt","in/file-007.txt","in/file-008.txt","in/file-009.txt"]],"image":null,"vcpu":1.0,"memory_mb":2048,"ttl_secs":60,"max_usd":null,"until":"all","concurrency":1,"profile":"basic","backend":null,"account":null,"region":null},"env":{"bucket":"example-bucket","queue_url":"https://sqs.us-west-2.amazonaws.com/111122223333/theseus-completions","cluster_arn":"arn:aws:ecs:us-west-2:111122223333:cluster/theseus-hands","log_group":"/theseus/hands","subnets":["subnet-0example0a","subnet-0example0b"],"security_group":"sg-0example","nat":false,"lambda_function":"arn:aws:lambda:us-west-2:111122223333:function:theseus-hand-basic","hand_image":"example.invalid/theseus/hand@sha256:00","execution_role_arn":"arn:aws:iam::111122223333:role/theseus-hand-execution","roles":{"basic":"arn:aws:iam::111122223333:role/theseus-hand-basic","read":"arn:aws:iam::111122223333:role/theseus-hand-read"},"lambda_memory_mb":2048},"task_definition":null,"hands":[{"index":0,"correlation_id":"act_00000000000000000000000000000332","external_op_id":"inv-6"}],"ttl_at":"2026-10-14T07:25:54Z","hand_max_usd":0.0020002040000000002,"created_at_ms":1791962694366,"endpoint":"http://127.0.0.1:34263","settled":null}"#;

#[test]
fn format_33_extension_and_hands_records_read_and_write_back_byte_for_byte() {
    let m: crate::extend::Manifest = serde_json::from_str(MANIFEST_BEFORE_HANDS).unwrap();
    assert!(m.capabilities.hands.is_none());
    assert_eq!(
        m.question_text(),
        "Load tidewc 3f2a1c: 1 tool, 1 of 1 test passed, no network?"
    );
    assert_eq!(serde_json::to_string(&m).unwrap(), MANIFEST_BEFORE_HANDS);
    let set: crate::extend::load::LoadedSet = serde_json::from_str(LOADED_BEFORE_HANDS).unwrap();
    assert!(set.loaded["tidewc"].capabilities.hands.is_none());
    assert_eq!(serde_json::to_string(&set).unwrap(), LOADED_BEFORE_HANDS);
    let g: crate::aws::hands::group::GroupRecord =
        serde_json::from_str(GROUP_BEFORE_EXTENSION).unwrap();
    assert!(g.extension.is_none());
    assert_eq!(serde_json::to_string(&g).unwrap(), GROUP_BEFORE_EXTENSION);
}

/// A store of format 33 opens, reads its extensions with no hands, takes an
/// extension's hands book, and moves the manifest to this build's format, which an older
/// build then refuses.
#[test]
fn a_format_33_store_opens_with_no_hands_and_takes_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    {
        let store = crate::store::Store::open(&path).unwrap();
        let set: crate::extend::load::LoadedSet =
            serde_json::from_str(LOADED_BEFORE_HANDS).unwrap();
        store.append(&[set.record().unwrap()]).unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 33, "engine": "redb"}"#).unwrap();
    let store = crate::store::Store::open(&path).unwrap();
    let set = crate::extend::load::LoadedSet::read(&store).unwrap();
    assert!(set.loaded["tidewc"].capabilities.hands.is_none());
    let book = crate::extend::broker::Book::default();
    store
        .put_meta(&crate::extend::broker::book_key("tidewc"), &book)
        .unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
}

/// A task from before projects reads as a plain item, with no date and no
/// review, and a project's three fields survive a round trip.
#[test]
fn a_format_44_task_reads_as_an_item_and_a_project_round_trips() {
    use theseus_protocol::tasks::{TaskKind, TaskRecord};
    let old: TaskRecord = serde_json::from_str(TASK_BEFORE_PROJECTS).unwrap();
    assert_eq!(old.kind, TaskKind::Item);
    assert_eq!((old.due_at_ms, old.review.as_deref()), (None, None));
    assert_eq!(serde_json::to_string(&old).unwrap(), TASK_BEFORE_PROJECTS);
    let mut project = old;
    project.kind = TaskKind::Project;
    project.review = Some("ses_00000000000000000000000000000321".into());
    project.due_at_ms = Some(1_790_100_000_000);
    let text = serde_json::to_string(&project).unwrap();
    assert!(text.contains(r#""kind":"project""#) && text.contains(r#""due_at_ms":1790100000000"#));
    assert_eq!(serde_json::from_str::<TaskRecord>(&text).unwrap(), project);
    let s: SessionRecord = serde_json::from_str(SESSION_BEFORE_PROJECT).unwrap();
    assert_eq!(s.project, None);
    assert_eq!(serde_json::to_string(&s).unwrap(), SESSION_BEFORE_PROJECT);
}

/// A format-47 session reads with no content labels (nothing judged, no
/// label: no migration) and writes back byte for byte; one with labels round
/// trips, and a write-back's merge of a record with none keeps none.
#[test]
fn a_format_47_session_reads_with_no_labels_and_takes_some() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    let mut s: SessionRecord = serde_json::from_str(SESSION_BEFORE_DATA).unwrap();
    assert!(s.data.is_none());
    assert_eq!(serde_json::to_string(&s).unwrap(), SESSION_BEFORE_DATA);
    let turn = s.clone();
    s.take_turns_fields(&turn);
    assert_eq!(serde_json::to_string(&s).unwrap(), SESSION_BEFORE_DATA);
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Secret,
        node: Some("msg_00000000000000000000000000000484".into()),
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.8,
        band: theseus_judge::band::Band::Confirm,
        at_ms: 1_790_000_000_490,
        judgment: String::new(),
    });
    s.data = Some(Box::new(flags));
    let text = serde_json::to_string(&s).unwrap();
    assert!(
        text.contains(r#","data":{"labels":[{"flag":"secret","node":"msg_00000000000000000000000000000484","kind":"tool_result""#)
            && text.ends_with(r#""at_ms":1790000000490}]}}"#),
        "{text}"
    );
    assert_eq!(
        serde_json::from_str::<SessionRecord>(&text).unwrap().data,
        s.data
    );
}

/// Test 29 of theseus-9ymz0: a store of format 51 holds a session whose `data`
/// is the open view's (the generated sample): the build opens it, reads the
/// record with empty content flags (no label, nothing judged: a stale open run
/// proves nothing under this rule), and its first write moves the manifest to
/// 52, which an older build then refuses.
#[test]
fn a_format_51_session_reads_its_open_view_data_as_empty_content_flags() {
    use crate::content::ContentFlags;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let old: SessionRecord = serde_json::from_str(SESSION_WITH_OPEN_VIEW_DATA).unwrap();
    {
        let store = crate::store::Store::open(&path).unwrap();
        store
            .append(&[theseus_store::NewRecord::bytes(
                kinds::SESSION,
                Some(&old.session_id),
                SESSION_WITH_OPEN_VIEW_DATA.as_bytes().to_vec(),
            )])
            .unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 51, "engine": "redb"}"#).unwrap();
    let store = crate::store::Store::open(&path).unwrap();
    let read: SessionRecord = store.get_session(&old.session_id).unwrap().unwrap();
    let flags = read.data.as_deref().expect("the old layout reads");
    assert_eq!(*flags, ContentFlags::default(), "no labels");
    assert_eq!(flags.disqualifies_cn(), None);
    assert!(!flags.frozen());
    assert_eq!(
        read.title.as_deref(),
        Some("Chart the breakwater"),
        "the rest reads"
    );
    // The first write moves the manifest.
    store.put_session(&read.session_id, &read).unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
}

/// Test of theseus-gyjyo.3: a store of format 52 holds a session whose labels
/// name no judgment (the generated sample): the build opens it, reads the
/// labels with an empty judgment (so they teach nothing), and its first write
/// moves the manifest to this build's, which an older build then refuses. The
/// sample's `judged_through` is the mark, which went at 54: ignored, and not
/// written back.
#[test]
fn a_format_52_session_reads_its_labels_with_no_judgment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let old: SessionRecord = serde_json::from_str(SESSION_WITH_LABELS_AT_52).unwrap();
    {
        let store = crate::store::Store::open(&path).unwrap();
        store
            .append(&[theseus_store::NewRecord::bytes(
                kinds::SESSION,
                Some(&old.session_id),
                SESSION_WITH_LABELS_AT_52.as_bytes().to_vec(),
            )])
            .unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 52, "engine": "redb"}"#).unwrap();
    let store = crate::store::Store::open(&path).unwrap();
    let read: SessionRecord = store.get_session(&old.session_id).unwrap().unwrap();
    let flags = read.data.as_deref().expect("the old layout reads");
    let secret = flags
        .label(crate::content::Flag::Secret)
        .expect("the label reads");
    assert_eq!(secret.judgment, "", "no judgment: it teaches nothing");
    assert_eq!(flags.removed.len(), 1, "the removal reads");
    assert_eq!(
        serde_json::to_string(&read).unwrap(),
        SESSION_WITH_LABELS_AT_52.replace(
            r#","judged_through":"msg_00000000000000000000000000000486""#,
            ""
        ),
        "and writes back every byte but the mark"
    );
    store.put_session(&read.session_id, &read).unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
}

/// Test of theseus-gyjyo.7: a store of format 53 holds a session whose `data`
/// carries the whole mark, `judged_through` and the `unjudged` worklist (the
/// generated sample), beside a label that names its judgment and a removal.
/// The build opens it, reads the labels and the removal and **ignores the
/// mark**: the session is not treated as partly judged, a request bound for a
/// model of restricted origin goes on the label alone, and the first write
/// moves the manifest to 54 and leaves the mark out of the record for good.
#[test]
fn a_format_53_session_with_a_mark_reads_it_ignored_and_drops_it() {
    use crate::content::Flag;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let old: SessionRecord = serde_json::from_str(SESSION_WITH_MARK_AT_53).unwrap();
    {
        let store = crate::store::Store::open(&path).unwrap();
        store
            .append(&[theseus_store::NewRecord::bytes(
                kinds::SESSION,
                Some(&old.session_id),
                SESSION_WITH_MARK_AT_53.as_bytes().to_vec(),
            )])
            .unwrap();
    }
    let manifest = path.join("MANIFEST.json");
    std::fs::write(&manifest, r#"{"format": 53, "engine": "redb"}"#).unwrap();
    let store = crate::store::Store::open(&path).unwrap();
    let read: SessionRecord = store.get_session(&old.session_id).unwrap().unwrap();
    let flags = read.data.as_deref().expect("the old layout reads");
    assert_eq!(flags.labels.len(), 1, "the label reads");
    assert_eq!(
        flags.label(Flag::Secret).unwrap().judgment,
        "jdg_0000000000000000000000000000048a",
        "with its judgment"
    );
    assert_eq!(flags.removed.len(), 1, "the removal reads");
    assert_eq!(
        flags.disqualifies_cn(),
        Some(Flag::Secret),
        "on the label alone"
    );
    let written = serde_json::to_string(&read).unwrap();
    assert!(
        !written.contains("judged_through") && !written.contains("unjudged"),
        "the mark is not written back: {written}"
    );
    store.put_session(&read.session_id, &read).unwrap();
    let moved: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(moved["format"], 55);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    let back: SessionRecord = again.get_session(&old.session_id).unwrap().unwrap();
    assert_eq!(
        back.data, read.data,
        "and the record reads the same after the write"
    );
}
