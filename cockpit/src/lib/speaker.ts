// Who a reply is from (theseus-slvl.3): "Theseus", with the model it ran on as metadata. The persona's own name and
// emoji replace the literal when persona data joins. Mirrors theseus-protocol's `surface::reply_label`, which
// `theseus history` prints.

export const REPLY_NAME = 'Theseus'

/** A reply's speaker and the model it ran on, `claude-` prefix dropped: the vendor's, which a reply's line need not carry. */
export function replySpeaker(model?: string | null): { name: string; model: string } {
  return { name: REPLY_NAME, model: (model ?? '').replace(/^claude-/, '') }
}
