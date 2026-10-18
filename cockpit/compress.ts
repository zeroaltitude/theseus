import { brotliCompressSync, constants, gzipSync } from 'node:zlib'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { extname, join } from 'node:path'
import type { Plugin } from 'vite'

// Formats that are compressed already: a second pass would only add bytes.
const PACKED = new Set(['.woff2', '.png', '.jpg', '.jpeg', '.webp', '.gif', '.br', '.gz'])

// theseusd embeds the build as `<file>.br` and `<file>.gz` beside each file it can shrink (theseus-rwpbg.5) and
// serves one by Accept-Encoding. Node's own zlib, so no package: brotli at quality 11 and gzip at level 9, and the
// gzip header carries no time (node writes mtime 0), so the same build makes the same bytes.
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
          writeFileSync(`${path}.gz`, gzipSync(bytes, { level: 9 }))
        }
      },
    },
  }
}
