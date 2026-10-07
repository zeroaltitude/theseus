// The living sea (theseus-hnof.2, the owner's C5): the swell carries the work, rising with real work, tokens a minute
// and the turns running, and settling as it ends. In Live mode it never stops: with nothing happening it rolls slowly,
// low, at a few frames a second (the owner, 2026-10-07, theseus-42ic), and the work raises it from there. Calm mode and
// reduced motion still it: dead calm, and the loop stops. Pure: the Ship computes the sea's height from its data, the
// engine eases toward it every frame, and a test holds both.

/** Tokens a minute at which the sea runs highest; below it the height grows with the logarithm, so a single model
 *  call's few hundred tokens show as a light swell and a cached long context's hundred thousand as a heavy sea. */
export const SEA_FULL_TPM = 60_000

/** The sea's height for the work now, 0 (dead calm) to 1 (a heavy sea): tokens a minute, and the turns running. Either
 *  raises it; both raise it more. */
export function seaTarget(tpm: number | null | undefined, turns: number): number {
  const t = Math.max(0, tpm ?? 0)
  const fromTokens = Math.min(1, Math.log10(1 + t / 150) / Math.log10(1 + SEA_FULL_TPM / 150))
  const fromTurns = 1 - Math.exp(-Math.max(0, turns) / 2)
  const v = 1 - (1 - fromTokens) * (1 - 0.75 * fromTurns)
  return v < 0.02 ? 0 : Math.min(1, v)
}

/** How far before the minute a scan of rows oldest first goes on: a model call's row may carry a time a little older
 *  than the rows before it, and a few such rows are passed over, not taken for the minute's start. */
const TPM_SLACK_MS = 10 * 60_000

/** Tokens a minute: input, cache and output of every model call (`provider.call` rows) since `now` (ms) less sixty
 *  seconds. The rows oldest first, as the ledger gives them: the scan starts from the newest and stops well before the
 *  minute, so the page's whole copy of the ledger costs no more than its last few minutes (the ambient sea's, on every
 *  page; the Ship's, on its own tail of model calls). */
export function tpmOf(rows: readonly { kind: string; at_unix_ms: number; data?: unknown }[], now: number): number {
  const cut = now - 60_000
  let sum = 0
  for (let i = rows.length - 1; i >= 0; i--) {
    const r = rows[i]
    if (r.at_unix_ms < cut - TPM_SLACK_MS) break
    if (r.kind !== 'provider.call' || r.at_unix_ms < cut) continue
    const u = ((r.data ?? {}) as Record<string, unknown>).usage as Record<string, unknown> | undefined
    if (!u) continue
    sum += Number(u.input_tokens ?? 0) + Number(u.output_tokens ?? 0) + Number(u.cache_read_input_tokens ?? 0) + Number(u.cache_creation_input_tokens ?? 0)
  }
  return sum
}

/** Seconds the sea takes to come up to its height (a time constant), and to settle back. */
export const SEA_RISE_S = 2.5
export const SEA_SETTLE_S = 8

/** The idle roll's height (theseus-42ic): Live mode's sea with nothing happening, a slow low swell. */
export const SEA_ROLL = 0.05

/** The sea's height in Live mode: the roll, raised by the work (any work raises it above the roll); and with the sea
 *  stilled (Calm, reduced motion, `?swell=0`) the work's height alone, which nothing draws. */
export function seaHeight(work: number, rolls: boolean): number {
  const w = Math.max(0, Math.min(1, work))
  return rolls ? SEA_ROLL + (1 - SEA_ROLL) * w : w
}

/** The sea's height after `dt` seconds of easing toward its target: up over a few seconds, down slower (it settles as
 *  the work ends) to the roll, where it stays exactly; on its way down to nothing (the sea stilled) it reaches dead
 *  calm, exactly 0, and the loop stops. The roll is already rolling: from dead calm (a page that opens, Calm let go)
 *  the sea starts at the roll, not easing up to it. */
export function seaStep(level: number, target: number, dt: number): number {
  if (target <= SEA_ROLL && level < target) return target
  const tau = target > level ? SEA_RISE_S : SEA_SETTLE_S
  const next = level + (target - level) * (1 - Math.exp(-Math.max(0, dt) / tau))
  if (target > 0 && Math.abs(next - target) < 0.002) return target
  return target === 0 && next < 0.01 ? 0 : next
}

/** The sea's state in a sailor's words, for the console and the key. */
export function seaWord(height: number): string {
  if (height <= 0) return 'dead calm'
  if (height <= SEA_ROLL) return 'a slow roll'
  if (height < 0.25) return 'a light swell'
  if (height < 0.55) return 'a moderate swell'
  if (height < 0.8) return 'a rough sea'
  return 'a heavy sea'
}

/** How fast the swell's clock runs at a height: slow crests in a light swell, quicker in a heavy sea. */
export const seaPace = (height: number) => 0.4 + 0.6 * Math.max(0, Math.min(1, height))
