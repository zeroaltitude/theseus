// An MCP server's prompt, run from the cockpit (M7 step 36c). The pure parts of the picker beside the composer:
// what to ask for, and what to send. The daemon checks again (a missing required argument is its refusal too); this
// only saves the round trip. Imports types only, so node's test runner loads it (`test/prompts.test.ts`).
import type { McpPromptInfo, McpPromptRef } from '@protocol'

/** The prompts a server lists, `server/prompt` order: what the picker's select shows. */
export function sortedPrompts(list: McpPromptInfo[]): McpPromptInfo[] {
  return [...list].sort((a, b) => a.name.localeCompare(b.name))
}

/** The names of the required arguments that `values` leaves empty. */
export function missingRequired(p: McpPromptInfo, values: Record<string, string>): string[] {
  return p.arguments.filter((a) => a.required && !(values[a.name] ?? '').trim()).map((a) => a.name)
}

/** The `turn.submit` prompt: the arguments filled in (an empty field is no argument), by name. */
export function promptRef(p: McpPromptInfo, values: Record<string, string>): McpPromptRef {
  const args: Record<string, string> = {}
  for (const a of p.arguments) {
    const v = values[a.name] ?? ''
    if (v.trim()) args[a.name] = v
  }
  return { server: p.server, name: p.prompt, arguments: args }
}

/** A field's label: a required argument is marked. */
export function fieldLabel(a: { name: string; required: boolean }): string {
  return a.required ? `${a.name} *` : a.name
}
