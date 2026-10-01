// Read-only JSON with folding and search, for tool inputs, results, manifests, and raw rows.
import { useMemo } from 'react'
import CodeMirror from '@uiw/react-codemirror'
import { json } from '@codemirror/lang-json'
import { EditorView } from '@codemirror/view'

const look = EditorView.theme({
  '&': { backgroundColor: 'transparent', fontSize: '12px' },
  '.cm-gutters': { backgroundColor: 'transparent', border: 'none', color: '#475569' },
  '.cm-content': { fontFamily: 'JetBrains Mono Variable, monospace' },
  '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'transparent' },
})

export function JsonView({ value, maxHeight = '320px' }: { value: unknown; maxHeight?: string }) {
  const text = useMemo(() => (typeof value === 'string' ? value : JSON.stringify(value, null, 2) ?? 'null'), [value])
  return (
    <div className="overflow-hidden rounded-md bg-black/30 ring-1 ring-line">
      <CodeMirror
        value={text}
        theme="dark"
        editable={false}
        readOnly
        maxHeight={maxHeight}
        basicSetup={{ lineNumbers: false, foldGutter: true, highlightActiveLine: false, highlightActiveLineGutter: false }}
        extensions={[json(), EditorView.lineWrapping, look]}
      />
    </div>
  )
}
