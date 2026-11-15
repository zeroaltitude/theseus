// Configuration's map (issue 17): every setting of Theseus that was found, said in plain words, and where it is changed.
// Three kinds, because that is the true division:
//   - LIVE: changed while Theseus runs, by a protocol method a page of the cockpit already calls, behind that page's own
//     confirmation. Its control is one of the Configuration tabs (the Ship's own component, never a copy) or a place
//     one link away. `methods` names the calls, so a test can hold the claim: each is a method of the protocol crate and
//     each is called from `src/`.
//   - FIXED: lives in the configuration file, which Theseus reads only when it starts (docs/setup.md). Changed by
//     editing the file and restarting; nothing changes it live. `tables` names the file's sections.
//   - NO_PAGE: changeable while running (the daemon has the method) but no page of the cockpit calls it yet, so there is
//     no control to put here without building a new path. Listed so the gap is visible, not hidden. A test holds that
//     no page calls them; when one does, the setting moves to LIVE.
// Pure: no imports but types, so `npm test` runs it.
import type { Health } from '@protocol'

/** The tabs of the Configuration page, in order (the page's `TABS` has the same ids). */
export const CONFIG_TABS = ['settings', 'personality', 'ontology', 'standing', 'policy', 'approvals', 'packs', 'system'] as const
export type ConfigTab = (typeof CONFIG_TABS)[number]

export interface LiveSetting {
  id: string; name: string; words: string
  /** Where the control is: a tab of this page, or a path (a section of the simple pages, or the Ship). */
  at: { tab: ConfigTab } | { path: string; label: string }
  methods: string[]
}

export const LIVE: LiveSetting[] = [
  { id: 'model', name: 'The model new work runs on', words: 'Which model profile Theseus uses for a new turn, unless the conversation names its own. Every conversation’s next turn changes model, so it asks first.', at: { tab: 'system' }, methods: ['profile.use'] },
  { id: 'conversation', name: 'One conversation’s model and mode', words: 'Choose the model a conversation uses from now on, or its mode. Open the conversation on Ops; its details hold the controls.', at: { path: '/', label: 'Ops' }, methods: ['session.profile', 'session.mode'] },
  { id: 'personas', name: 'Who Theseus is, place by place', words: 'The personas: add one, write its character, choose which is the default, retire one.', at: { tab: 'personality' }, methods: ['persona.add', 'persona.default', 'persona.retire', 'persona.soul'] },
  { id: 'ontology', name: 'What Theseus keeps track of', words: 'The kinds of thing and their categories, the guidance it reads for each, and the people: merge two, accept or turn down what Theseus proposes, and undo what it filed itself.', at: { tab: 'ontology' }, methods: ['ontology.kind.add', 'ontology.category.add', 'ontology.guidance.set', 'ontology.person.merge', 'ontology.proposal.accept_all', 'ontology.proposal.reject_all', 'ontology.proposal.never', 'ontology.never.undo', 'ontology.undo'] },
  { id: 'standing', name: 'The standing rules', words: 'The instructions Theseus carries into every turn. You can take one back.', at: { tab: 'standing' }, methods: ['standing.revoke'] },
  { id: 'tightenings', name: 'What Theseus must now ask about', words: 'Make a tool ask first when it did not, and undo that. These tightenings add to the policy in the configuration file and never loosen it.', at: { tab: 'approvals' }, methods: ['policy.tighten', 'policy.untighten'] },
  { id: 'families', name: 'Which tools each place loads', words: 'Each kind of place loads some families of tools and keeps others out of sight until they are needed. Change which.', at: { tab: 'approvals' }, methods: ['tool.defer'] },
  { id: 'approvals', name: 'Answering what waits for you', words: 'An approval, a question or a budget limit waits until you answer it, from here or anywhere else.', at: { tab: 'approvals' }, methods: ['work.answer'] },
  { id: 'extensions', name: 'Extensions', words: 'Take back the access an extension was given.', at: { tab: 'system' }, methods: ['extension.revoke'] },
  { id: 'self-halt', name: 'Stopping its work on itself', words: 'Halt everything Theseus is doing to improve itself, before its next step. Only the owner resumes it.', at: { tab: 'system' }, methods: ['self.halt'] },
  { id: 'packs', name: 'How its small judging model is tuned', words: 'Promote a newer version of a judging pack or roll one back, step by step along its ladder.', at: { tab: 'packs' }, methods: ['pack.promote', 'pack.rollback'] },
  { id: 'budgets', name: 'A budget that has been reached', words: 'When a conversation reaches its spend limit it stops and asks. The answer lets it go on.', at: { tab: 'approvals' }, methods: ['work.answer'] },
  { id: 'timeline', name: 'Wakes, routines and watches', words: 'Snooze, pause, resume, cancel or run a wake, a routine or a watch.', at: { path: '/ops?lens=timeline', label: 'the Ship’s Operations timeline' }, methods: ['wake.snooze', 'wake.pause', 'wake.resume', 'wake.cancel', 'routine.pause', 'routine.resume', 'routine.skip', 'routine.run_now', 'routine.delete'] },
  { id: 'project', name: 'Which project a conversation or task belongs to', words: 'Attach a conversation or a task to a project: drag it onto the project.', at: { path: '/ops', label: 'the Ship’s Operations page' }, methods: ['project.attach'] },
]

export interface FixedSetting {
  id: string; name: string; words: string
  /** The sections of the configuration file this lives in, as the file names them. */
  tables: string[]
  /** What Theseus is running with now, where it says so. */
  now?: (h: Health) => string | undefined
}

const usd = (n: number) => `$${n.toLocaleString('en-US', { maximumFractionDigits: 2 })}`

export const FIXED: FixedSetting[] = [
  { id: 'limits', name: 'Limits on work and spend', words: 'How many turns run at once, what one conversation may spend before it stops or asks, the day’s spend ceiling, and how long a call waits for your answer.', tables: ['kernel'],
    now: (h) => `${h.kernel.admission_ceiling} turns at once · ${usd(h.kernel.spend_limit_usd)} a conversation${h.kernel.spend_limit_mode ? `, then ${h.kernel.spend_limit_mode}` : ''}` },
  { id: 'models', name: 'Models and providers', words: 'The model profiles and which one is live at start, the providers and their timeouts and retries, the prompt cache, and the catalog of models and their prices.', tables: ['model', 'profiles', 'providers', 'cache', 'catalog'],
    now: (h) => `${h.model} on ${h.provider}, profile ${h.profile}` },
  { id: 'context', name: 'What every turn starts from', words: 'The context files, how large a prompt may grow, the personas’ files, and importing a persona from elsewhere.', tables: ['context', 'personas', 'persona_import'] },
  { id: 'secrets', name: 'Secrets and the broker', words: 'Which secrets Theseus needs, where each comes from, and the programs it may run with them.', tables: ['secrets', 'broker'] },
  { id: 'doors', name: 'The doors in', words: 'The server, the web page and who may open it, the terminal screen, this cockpit, and the tool server other programs may call.', tables: ['server', 'web', 'tui', 'cockpit', 'mcp_server'] },
  { id: 'tools', name: 'Tools', words: 'Which tools exist, web access, the terminal, the language servers, the tool servers Theseus starts, and the families deferred at start.', tables: ['tools', 'tools.web', 'tools.term', 'tools.defer', 'lsp', 'mcp'] },
  { id: 'rules', name: 'The base rules for what Theseus may do', words: 'The policy of each tool, of the tool servers and of the cloud, the sandbox, privacy, the check of what content may go where, and the places Theseus acts in. What you tighten live sits on top of these.', tables: ['policy', 'sandbox', 'privacy', 'content_based_security', 'places'] },
  { id: 'accounts', name: 'Accounts it works in', words: 'The cloud accounts, Google, GitHub, Discord and the voice.', tables: ['aws', 'google', 'github', 'discord', 'voice'] },
  { id: 'memory', name: 'What it remembers and how', words: 'The search index, memory, sessions and threads, the standing rules’ and people’s and ontology’s own settings, the cards, and what is imported from elsewhere.', tables: ['index', 'memory', 'sessions', 'threads', 'standing', 'people', 'ontology', 'card', 'import'] },
  { id: 'judging', name: 'Judging and routing', words: 'The small judging model, its packs and its learning, and how a message is routed and the corrections of the routing kept.', tables: ['judge', 'routing', 'route_bench', 'prefix'] },
  { id: 'initiative', name: 'What it does on its own', words: 'Its initiative, routines, watches and notices, and the projects it knows.', tables: ['initiative', 'routines', 'watch_for', 'notify', 'project', 'projects'] },
  { id: 'self', name: 'Improving itself', words: 'The self-improvement loop, its own spend account, the pattern miner, and the sealed exams kept out of its reach.', tables: ['self', 'self_issues'] },
  { id: 'telemetry', name: 'What it reports about itself', words: 'Whether and where Theseus sends its own telemetry.', tables: ['telemetry'] },
]

export interface NoPageSetting { id: string; name: string; words: string; methods: string[] }

export const NO_PAGE: NoPageSetting[] = [
  { id: 'trust', name: 'Which project folders Theseus trusts', words: 'Trust or stop trusting a project’s folder, and start a project.', methods: ['project.trust', 'project.untrust', 'project.create'] },
  { id: 'standing-add', name: 'Adding and answering standing rules', words: 'Add a standing rule, keep or dismiss one Theseus noticed, and answer its question about one. Only taking one back has a page.', methods: ['standing.add', 'standing.keep', 'standing.dismiss', 'standing.answer'] },
  { id: 'google', name: 'Signing in to Google', words: 'Begin and finish the sign-in, and take the access back.', methods: ['google.auth.begin', 'google.auth.finish', 'google.revoke'] },
  { id: 'servers', name: 'Restarting a tool server', words: 'Restart one of the tool servers Theseus started.', methods: ['mcp.restart'] },
  { id: 'index', name: 'Rebuilding the search index', words: 'Build the memory’s search index again from the record.', methods: ['index.rebuild'] },
  { id: 'import', name: 'Erasing what was imported', words: 'Erase every conversation that one import brought in.', methods: ['import.erase'] },
  { id: 'judge', name: 'Teaching and proving the judging model', words: 'Start its learning, prove a version, and fill in the labels it missed.', methods: ['judge.learn', 'judge.prove', 'judge.backfill'] },
  { id: 'pins', name: 'Letting go of a pinned model', words: 'Release the live model, or unpin a conversation’s.', methods: ['profile.release', 'profile.unpin'] },
  { id: 'persona-set', name: 'Importing a persona and setting its fields', words: 'Bring a persona in from elsewhere, or change a persona’s settings.', methods: ['persona.import', 'persona.set'] },
  { id: 'kinds', name: 'Retiring a kind, and answering one proposal at a time', words: 'Retire a kind of thing, accept or reject one proposal, not all.', methods: ['ontology.kind.retire', 'ontology.proposal.accept', 'ontology.proposal.reject'] },
  { id: 'self-pause', name: 'Pausing and resuming its work on itself', words: 'Pause new builds of its own changes, or resume after a halt. Resuming is the owner’s alone, from the command line.', methods: ['self.pause', 'self.resume'] },
  { id: 'initiative-rate', name: 'Rating what Theseus did on its own', words: 'Say whether something Theseus did unprompted was welcome.', methods: ['initiative.rate'] },
]

/** Every table the file's example names that a group of FIXED does not (the test reads the example and lists what is
 *  missing, so a new section of the file cannot be left off the page). The head of a dotted name is its table. */
export const tableHead = (t: string) => t.split('.')[0]
