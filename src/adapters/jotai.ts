import { atomWithStorage } from 'jotai/utils'
import {
  getDefaultKV,
  INTERNAL_GET_JSON_TEXT,
  INTERNAL_IS_CLOSED,
  INTERNAL_ON_KEY_CHANGED,
  INTERNAL_SET_JSON,
  type KV,
  type KVSubscription,
} from '../kv'

/** Atom persisted in a KV store: reads synchronously on init, follows writes from any handle, and `undefined` deletes. */
export function atomWithKV<T>(key: string, initialValue: T, kv?: KV) {
  const store = () => kv ?? getDefaultKV()
  // The change event of our own write must hand back the written object, not a reparsed copy.
  let written: { json: string | undefined; value: T } | undefined
  return atomWithStorage<T>(
    key,
    initialValue,
    {
      getItem: (k, fallback) => {
        const value = store().getJSON<T>(k)
        return value === undefined ? fallback : value
      },
      setItem: (k, value) => {
        if (value === undefined) {
          written = { json: undefined, value }
          store().delete(k)
          return
        }
        written = { json: store()[INTERNAL_SET_JSON](k, value), value }
      },
      removeItem: (k) => {
        written = undefined
        store().delete(k)
      },
      subscribe: (k, callback, fallback) => {
        let subscription: KVSubscription
        const listen = () => {
          const target = store()
          subscription = target[INTERNAL_ON_KEY_CHANGED](k, () => {
            if (kv === undefined && target[INTERNAL_IS_CLOSED]) listen()
            const json = store()[INTERNAL_GET_JSON_TEXT](k)
            if (written !== undefined && written.json === json) {
              callback(written.value)
              return
            }
            written = undefined
            callback(json === undefined ? fallback : (JSON.parse(json) as T))
          })
        }
        listen()
        return () => subscription.remove()
      },
    },
    { getOnInit: true }
  )
}
