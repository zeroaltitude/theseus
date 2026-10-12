// A view pref kept in this browser: read once, written on every change; with no storage it lasts as long as the page.
import { useCallback, useState } from 'react'

export function usePref<T extends string>(key: string, parse: (raw: string | null) => T): [T, (v: T) => void] {
  const [value, set] = useState<T>(() => {
    try { return parse(localStorage.getItem(key)) } catch { return parse(null) }
  })
  const put = useCallback((v: T) => {
    set(v)
    try { localStorage.setItem(key, v) } catch { /* the page's state stands */ }
  }, [key])
  return [value, put]
}
