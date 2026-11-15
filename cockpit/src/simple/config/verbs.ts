// The Configuration controls' verb table (issue 17, step 5): every act of the "More controls" tab, the protocol method it
// calls and the posture it deserves, in the Operations page's own words for postures (`ops/verbs.ts`): `do` one press and
// a line saying what happened; `arm` press twice; `ask` a sheet that names exactly what will happen first. One table:
// `ActButton` reads the posture from here, so a control cannot choose a lighter one than its weight, and
// `test/simple-config-controls.test.ts` pins each row to its method and posture and holds that every destroying act asks.
// The Operations table has its own rows and is pinned by its own test; these are the daemon-level settings it has no row
// for. Pure: types only.
import type { Posture, VerbDef } from '../../ops/verbs.ts'

const on = ['daemon'] as const

export const CONFIG_VERBS: readonly VerbDef[] = [
  // Projects: which repositories Theseus trusts, and starting one.
  { id: 'project_trust', label: 'Trust a project’s folder', on, methods: ['project.trust'], posture: 'ask' },
  { id: 'project_untrust', label: 'Stop trusting a project’s folder', on, methods: ['project.untrust'], posture: 'arm' },
  { id: 'project_create', label: 'Start a project', on, methods: ['project.create'], posture: 'ask' },
  // Standing rules: add one, and answer what Theseus noticed.
  { id: 'standing_add', label: 'Add a standing rule', on, methods: ['standing.add'], posture: 'ask' },
  { id: 'standing_keep', label: 'Keep a proposed standing rule', on, methods: ['standing.keep'], posture: 'ask' },
  { id: 'standing_dismiss', label: 'Dismiss a proposed standing rule', on, methods: ['standing.dismiss'], posture: 'do' },
  { id: 'standing_answer', label: 'Answer a question about a standing rule', on, methods: ['standing.answer'], posture: 'arm' },
  // The tool servers, the search index, and the pinned models.
  { id: 'mcp_restart', label: 'Restart a tool server', on, methods: ['mcp.restart'], posture: 'ask' },
  { id: 'index_rebuild', label: 'Rebuild the search index', on, methods: ['index.rebuild'], posture: 'ask' },
  { id: 'profile_release', label: 'Release the live model', on, methods: ['profile.release'], posture: 'ask' },
  { id: 'profile_unpin', label: 'Unpin every model', on, methods: ['profile.unpin'], posture: 'ask' },
  // Imports: erase what one import brought in. Cannot be undone.
  { id: 'import_erase', label: 'Erase an import', on, methods: ['import.erase'], posture: 'ask' },
  // Judging: learn now, read the proof, fill in what was missed (which costs money).
  { id: 'judge_learn', label: 'Run the learning loop now', on, methods: ['judge.learn'], posture: 'ask' },
  { id: 'judge_prove', label: 'Read the proof of the judging model', on, methods: ['judge.prove'], posture: 'do' },
  { id: 'judge_backfill', label: 'Judge past events the model missed', on, methods: ['judge.backfill'], posture: 'ask' },
  // Personas: bring one in (a preview first) and change a persona’s fields.
  { id: 'persona_preview', label: 'Preview a persona import', on, methods: ['persona.import'], posture: 'do' },
  { id: 'persona_import', label: 'Import a persona', on, methods: ['persona.import'], posture: 'ask' },
  { id: 'persona_set', label: 'Change a persona’s fields', on, methods: ['persona.set'], posture: 'do' },
  // The ontology: retire a kind, and answer one proposal at a time.
  { id: 'kind_retire', label: 'Retire a kind', on, methods: ['ontology.kind.retire'], posture: 'ask' },
  { id: 'proposal_accept', label: 'Accept a proposal', on, methods: ['ontology.proposal.accept'], posture: 'arm' },
  { id: 'proposal_reject', label: 'Reject a proposal', on, methods: ['ontology.proposal.reject'], posture: 'do' },
  // What Theseus does on its own: rate it, and pause or resume its self-improvement builds.
  { id: 'initiative_up', label: 'Say a message Theseus sent first was welcome', on, methods: ['initiative.rate'], posture: 'do' },
  { id: 'initiative_down', label: 'Say a message Theseus sent first was not welcome', on, methods: ['initiative.rate'], posture: 'do' },
  { id: 'self_pause', label: 'Pause new self-improvement builds', on, methods: ['self.pause'], posture: 'do' },
  { id: 'self_unpause', label: 'Let self-improvement builds start again', on, methods: ['self.pause'], posture: 'arm' },
  { id: 'self_resume', label: 'Resume after a halt', on, methods: ['self.resume'], posture: 'ask' },
]

export const configVerb = (id: string): VerbDef => {
  const v = CONFIG_VERBS.find((x) => x.id === id)
  if (!v) throw new Error(`no config verb ${id}`)
  return v
}

/** The acts that destroy something or cannot be taken back: each must be `ask`, in words that name the effect. */
export const DESTROYS: readonly string[] = ['import_erase', 'index_rebuild', 'kind_retire', 'mcp_restart', 'project_trust', 'profile_unpin', 'self_resume']

export const postureOf = (id: string): Posture => configVerb(id).posture
