import { getDefaultKV, type KV } from '../kv'

/**
 * redux-persist storage engine over a KV instance:
 *
 * persistReducer({ key: 'root', storage: createSccStorage() }, rootReducer)
 */
export function createSccStorage(kv?: KV) {
  const store = kv ?? getDefaultKV()
  return {
    getItem: async (key: string): Promise<string | null> =>
      store.getString(key) ?? null,
    setItem: async (key: string, value: string): Promise<void> => {
      store.set(key, value)
    },
    removeItem: async (key: string): Promise<void> => {
      store.delete(key)
    },
  }
}
