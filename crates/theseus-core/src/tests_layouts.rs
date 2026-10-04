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

const SAMPLES: &[Sample] = &[
    Sample {
        kind: kinds::SESSION,
        layout: "1, 460a35b's: before T1's hold on external text (2), its run of failures (3), the images its provider refused (4), a search's query (5), and the 1-hour cache writes (6)",
        bytes: r#"{"session_id":"ses_01a0f3f3c9b77474a7caa5d4925bd678","kind":"conversation","label":null,"created_at_unix_ms":1790799235511,"turns":1,"last_turn_id":"turn_01a0f3f3cd4774a9a2462c729fb82f1b","usage":{"input_tokens":100,"output_tokens":24,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"execution_id":"exe_01a0f3f3c9b77474a7caa5d505b05b5e","compilation_id":"cmp_01a0f3f3cd57766c8225231563419be3","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790799237649,"cost_usd":0.00044,"tool_calls":1,"title":"Start the background job."}"#,
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
        layout: "a hand a cancel reached at format 7 (step 40 part 1): unsupported, as an in-process call, before a hand's own stop and ECS's verdict (8, theseus-mgw.11)",
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
        layout: "1, 460a35b's: an assistant message that calls a tool",
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
        layout: "a row of a kind no build writes now (the place rule, theseus-nbsh): 19a's or 19c's label.held_post_answered",
        bytes: r#"{"at_unix_ms":1790000000075,"kind":"label.held_post_answered","session_id":"ses_lighthouse","data":{"approve":false,"by":"the CLI","question":"act_00000000000000000000000000000074"}}"#,
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
