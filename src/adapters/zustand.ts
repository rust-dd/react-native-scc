import type { StateStorage } from 'zustand/middleware'
import { getDefaultKV, type KV } from '../kv'

/**
 * zustand persist storage over a KV instance. Synchronous, so
 * hydration completes without an async gap:
 *
 * persist(config, {
 *   name: 'my-store',
 *   storage: createJSONStorage(() => sccStateStorage()),
 * })
 */
export function sccStateStorage(kv?: KV): StateStorage {
  const store = kv ?? getDefaultKV()
  return {
    getItem: (name) => store.getString(name) ?? null,
    setItem: (name, value) => {
      store.set(name, value)
    },
    removeItem: (name) => {
      store.delete(name)
    },
  }
}
