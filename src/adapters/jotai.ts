import { atomWithStorage } from 'jotai/utils'
import { getDefaultKV, type KV } from '../kv'

/**
 * Atom persisted in a KV instance. Reads synchronously on init and
 * reacts to writes made outside jotai (other KV handles included).
 */
export function atomWithKV<T>(key: string, initialValue: T, kv?: KV) {
  const store = kv ?? getDefaultKV()
  return atomWithStorage<T>(
    key,
    initialValue,
    {
      getItem: (k, fallback) => {
        const value = store.getJSON<T>(k)
        return value === undefined ? fallback : value
      },
      setItem: (k, value) => {
        store.setJSON(k, value)
      },
      removeItem: (k) => {
        store.delete(k)
      },
      subscribe: (k, callback, fallback) => {
        const subscription = store.addOnValueChangedListener((changed) => {
          if (changed === null || changed === k) {
            const value = store.getJSON<T>(k)
            callback(value === undefined ? fallback : value)
          }
        })
        return () => subscription.remove()
      },
    },
    { getOnInit: true }
  )
}
