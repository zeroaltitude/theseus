// The living sea (theseus-hnof.2, the owner's C5): the swell carries the work. Dead calm when nothing happens (the
// render loop stops: an idle Ship draws nothing), rising with real work, tokens a minute and the turns running, and
// settling as it ends. Calm mode and reduced motion still it. Pure: the Ship computes the sea's height from its data,
// the engine eases toward it every frame, and a test holds both.

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

/** Seconds the sea takes to come up to its height (a time constant), and to settle back. */
export const SEA_RISE_S = 2.5
export const SEA_SETTLE_S = 8

/** The sea's height after `dt` seconds of easing toward its target: up over a few seconds, down slower (it settles as
 *  the work ends); on its way down to nothing it reaches dead calm, exactly 0, and the loop stops. */
export function seaStep(level: number, target: number, dt: number): number {
  const tau = target > level ? SEA_RISE_S : SEA_SETTLE_S
  const next = level + (target - level) * (1 - Math.exp(-Math.max(0, dt) / tau))
  return target === 0 && next < 0.01 ? 0 : next
}

/** The sea's state in a sailor's words, for the console and the key. */
export function seaWord(height: number): string {
  if (height <= 0) return 'dead calm'
  if (height < 0.25) return 'a light swell'
  if (height < 0.55) return 'a moderate swell'
  if (height < 0.8) return 'a rough sea'
  return 'a heavy sea'
}

/** How fast the swell's clock runs at a height: slow crests in a light swell, quicker in a heavy sea. */
export const seaPace = (height: number) => 0.4 + 0.6 * Math.max(0, Math.min(1, height))
