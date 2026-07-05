import { getDefaultKV, type KV } from '../kv'

/**
 * redux-persist storage engine over a KV instance:
 *
 * persistReducer({ key: 'root', storage: createSccStorage() }, rootReducer)
 */
export function createSccStorage(kv?: KV) {
  const store = kv ?? getDefaultKV()
  return {
    getItem: (key: string): Promise<string | null> =>
      Promise.resolve(store.getString(key) ?? null),
    setItem: (key: string, value: string): Promise<void> => {
      store.set(key, value)
      return Promise.resolve()
    },
    removeItem: (key: string): Promise<void> => {
      store.delete(key)
      return Promise.resolve()
    },
  }
}
