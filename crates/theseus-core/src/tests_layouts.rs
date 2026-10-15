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

const SAMPLES: &[Sample] = &[
    Sample {
        kind: kinds::COMPILATION,
        layout: "a compilation at format 32 (theseus-2qzx's): before its manifest's loaded tool families (36, theseus-7m7w.2)",
        bytes: COMPILATION_BEFORE_LOADED,
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
        // categories (`onto:cat:`).
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
    assert_eq!(moved["format"], 40);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    assert_eq!(
        *crate::standing::Board::default().all(&again).unwrap(),
        vec![r]
    );
}

/// A store of format 29, whose ledger holds rows with no chain (theseus-bmkxk):
/// the build opens it and reads them, its first ledger write starts the chain
/// with a row that counts and pins them, and the manifest moves to 40. The
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
    assert_eq!(moved["format"], 40);
    let after = theseus_store::chain_walk::verify(store.inner().as_ref(), None, None).unwrap();
    assert!(after.broken.is_none(), "{after:?}");
    assert_eq!((after.rows, after.unchained), (2, 1));
}

/// A store of format 29, which holds a session and a compilation as the
/// build before forks (theseus-2qzx) wrote them: the build opens it, reads
/// both with no origin and no inherited run, and moves the manifest to 40,
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
    assert_eq!(moved["format"], 40);
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

/// A store of format 32, which holds a session and no thread: the build opens
/// it, reads an empty board, takes a thread and moves the manifest to 40,
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
    assert_eq!(moved["format"], 40);
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
    assert_eq!(moved["format"], 40);
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

/// A store of format 33 opens, and its first write moves the manifest to 40,
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
    assert_eq!(moved["format"], 40);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    assert_eq!(again.get_compilation(&c.id).unwrap().unwrap(), c);
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
/// reads the record, takes a surface-scoped one, and moves the manifest to 40,
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
    assert_eq!(moved["format"], 40);
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
/// opens it, takes a pre-image, and moves the manifest to 40, which an older
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
    assert_eq!(moved["format"], 40);
    drop(store);
    let again = crate::store::Store::open(&path).unwrap();
    let rows = crate::rewind::rows(&again, "ses_harbour").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, p);
}
