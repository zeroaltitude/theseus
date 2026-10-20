import { brotliCompressSync, constants } from 'node:zlib'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { extname, join } from 'node:path'
import type { Plugin } from 'vite'

// Formats that are compressed already: a second pass would only add bytes.
const PACKED = new Set(['.woff2', '.png', '.jpg', '.jpeg', '.webp', '.gif', '.br'])

// theseusd embeds the build as `<file>.br` beside each file it can shrink (theseus-rwpbg.5), one copy (theseus-o2myi),
// and sends it to a client that takes brotli, else decoded. Node's own zlib, so no package: brotli at quality 11.
export function compressed(): Plugin {
  let out = ''
  return {
    name: 'theseus-compressed',
    apply: 'build',
    configResolved(c) {
      out = join(c.root, c.build.outDir)
    },
    closeBundle: {
      order: 'post',
      handler() {
        for (const e of readdirSync(out, { recursive: true, withFileTypes: true })) {
          if (!e.isFile() || PACKED.has(extname(e.name))) continue
          const path = join(e.parentPath, e.name)
          const bytes = readFileSync(path)
          writeFileSync(`${path}.br`, brotliCompressSync(bytes, {
            params: {
              [constants.BROTLI_PARAM_QUALITY]: 11,
              [constants.BROTLI_PARAM_SIZE_HINT]: bytes.length,
            },
          }))
        }
      },
    },
  }
}
