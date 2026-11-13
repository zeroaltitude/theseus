// A file the owner attaches to a message from the composer (theseus-qgsv): the browser's half of what
// `theseus ask --attach` and the chat's `@image.png` build, which is `crates/theseus/src/attach.rs`'s
// `attachment_labeled`, clause for clause:
//
//   * a text file travels as `text` with media type `text/plain`; a notebook and an RTF file are text the
//     daemon reads as a document, so they travel as bytes, as that builder sends them;
//   * any other file travels as base64 `data` with no media type: the daemon sniffs the bytes and keeps an
//     image as an image, reads a PDF, and names anything else;
//   * a file over the cap is not read: it carries `not_read` with the reason, and the composer refuses to
//     send it in the same words the chat's `@image` refuses one (`<name>: over the 32 MiB limit for …`).
//
// Pure: the page reads the bytes (`File.arrayBuffer()`) and hands them here. Tested by
// `test/ops-composer.test.ts`.
import type { Attachment } from '@protocol'

/** The largest file the composer sends: the daemon's own default for `[tools] max_attachment_bytes`
 *  (theseus-c9l6), which it applies again. The cap is not on the wire, so this mirrors the CLI's
 *  `MAX_ATTACH_BYTES`. */
export const MAX_ATTACH_BYTES = 32 * 1024 * 1024

/** The words the cap's refusal uses, the CLI's with the act's own label. */
export const overCap = (label: string) => `over the 32 MiB limit for ${label}`

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'

/** The standard alphabet, padded, as Rust's `base64::engine::general_purpose::STANDARD` writes it. */
export function base64(bytes: Uint8Array): string {
  let out = ''
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i], b = bytes[i + 1], c = bytes[i + 2]
    const n = (a << 16) | ((b ?? 0) << 8) | (c ?? 0)
    out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63]
    out += b === undefined ? '=' : B64[(n >> 6) & 63]
    out += c === undefined ? '=' : B64[n & 63]
  }
  return out
}

/** Text, by the builder's own rule: the bytes decode as UTF-8 and hold no NUL in their first 8 KiB. */
function textOf(bytes: Uint8Array): string | null {
  if (bytes.subarray(0, 8192).includes(0)) return null
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes)
  } catch {
    return null
  }
}

const AS_BYTES = ['ipynb', 'rtf']

/** One chosen file as `turn.submit` carries it. `size` is the file's own, so a file over the cap is listed
 *  with the reason and its bytes are never read; `label` is the act the refusal names. */
export function attachmentOf(file: { name: string; size: number }, bytes: Uint8Array, label = 'the composer'): Attachment {
  const a: Attachment = { name: file.name, media_type: '', size: file.size }
  if (file.size > MAX_ATTACH_BYTES || bytes.length > MAX_ATTACH_BYTES) return { ...a, not_read: overCap(label) }
  const ext = file.name.split('.').pop()?.toLowerCase() ?? ''
  const text = AS_BYTES.includes(ext) ? null : textOf(bytes)
  if (text != null) return { ...a, media_type: 'text/plain', text }
  return { ...a, data: base64(bytes) }
}

/** Why the message cannot go, in the chat's words, or null when every file was read. */
export function attachRefusal(attachments: readonly Attachment[]): string | null {
  const bad = attachments.find((a) => a.not_read)
  return bad ? `${bad.name}: ${bad.not_read}` : null
}
