import { useEffect, useInsertionEffect, useRef, useState } from 'react'
import { getDefaultKV, KV } from './kv'

type Reader<T> = (kv: KV, key: string) => T | undefined

function useKVValue<T>(
  key: string,
  kv: KV | undefined,
  read: Reader<T>,
  write: (kv: KV, key: string, value: T) => void
): [T | undefined, (value: T | undefined) => void] {
  const store = kv ?? getDefaultKV()
  const [value, setValue] = useState<T | undefined>(() => read(store, key))

  useEffect(() => {
    setValue(read(store, key))
    const subscription = store.addOnValueChangedListener((changedKey) => {
      if (changedKey === null || changedKey === key) {
        setValue(read(store, key))
      }
    })
    return () => subscription.remove()
    // `read`/`write` are module-level constants at every call site.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [store, key])

  const set = (next: T | undefined) => {
    if (next === undefined) store.delete(key)
    else write(store, key, next)
  }

  return [value, set]
}

const readString: Reader<string> = (kv, key) => kv.getString(key)
const readNumber: Reader<number> = (kv, key) => kv.getNumber(key)
const readBoolean: Reader<boolean> = (kv, key) => kv.getBoolean(key)
const readBuffer: Reader<ArrayBuffer> = (kv, key) => kv.getBuffer(key)

export function useKVString(
  key: string,
  kv?: KV
): [string | undefined, (value: string | undefined) => void] {
  return useKVValue(key, kv, readString, (s, k, v) => s.set(k, v))
}

export function useKVNumber(
  key: string,
  kv?: KV
): [number | undefined, (value: number | undefined) => void] {
  return useKVValue(key, kv, readNumber, (s, k, v) => s.set(k, v))
}

export function useKVBoolean(
  key: string,
  kv?: KV
): [boolean | undefined, (value: boolean | undefined) => void] {
  return useKVValue(key, kv, readBoolean, (s, k, v) => s.set(k, v))
}

export function useKVBuffer(
  key: string,
  kv?: KV
): [ArrayBuffer | undefined, (value: ArrayBuffer | undefined) => void] {
  return useKVValue(key, kv, readBuffer, (s, k, v) => s.set(k, v))
}

export function useKVJSON<T = unknown>(
  key: string,
  kv?: KV
): [T | undefined, (value: T | undefined) => void] {
  return useKVValue<T>(
    key,
    kv,
    (s, k) => s.getJSON<T>(k),
    (s, k, v) => s.setJSON(k, v)
  )
}

export function useKVSelector<T = unknown, S = T | undefined>(
  key: string,
  selector: (value: T | undefined) => S,
  kv?: KV,
  equals: (a: S, b: S) => boolean = Object.is
): S {
  const store = kv ?? getDefaultKV()
  const [value, setValue] = useState<S>(() => selector(store.getJSON<T>(key)))

  // Refs (not effect deps) so an inline selector's fresh identity each render
  // doesn't resubscribe the native listener — or, for object selectors, loop
  // via observeJSON's immediate emit. Written in useInsertionEffect so an
  // abandoned concurrent render can't leave a stale closure in the ref.
  const selectorRef = useRef(selector)
  const equalsRef = useRef(equals)
  useInsertionEffect(() => {
    selectorRef.current = selector
    equalsRef.current = equals
  })

  useEffect(() => {
    const subscription = store.observeJSON<T, S>(
      key,
      (v) => selectorRef.current(v),
      setValue,
      (a, b) => equalsRef.current(a, b)
    )
    return () => subscription.remove()
  }, [store, key])

  return value
}
