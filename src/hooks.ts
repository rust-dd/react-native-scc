import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useSyncExternalStore,
} from 'react'
import { getDefaultKV, INTERNAL_GET_JSON_TEXT, KV } from './kv'

interface Snapshot<T> {
  value: T | undefined
}

type SnapshotReader<T> = (
  kv: KV,
  key: string,
  previous?: Snapshot<T>
) => Snapshot<T>
type SnapshotEquals<T> = (previous: Snapshot<T>, next: Snapshot<T>) => boolean

class KVSnapshotSource<T> {
  private snapshot: Snapshot<T> | undefined
  private readonly listeners = new Set<() => void>()
  private subscription: { remove(): void } | undefined

  constructor(
    private readonly store: KV,
    private readonly key: string,
    private readonly read: SnapshotReader<T>,
    private readonly equals: SnapshotEquals<T>
  ) {}

  readonly getSnapshot = (): Snapshot<T> => {
    const next = this.read(this.store, this.key, this.snapshot)
    if (this.snapshot === undefined || !this.equals(this.snapshot, next)) {
      this.snapshot = next
    }
    return this.snapshot
  }

  readonly subscribe = (listener: () => void): (() => void) => {
    const before = this.getSnapshot()
    this.listeners.add(listener)
    try {
      this.subscription ??= this.store.addOnValueChangedListener((changedKey) => {
        if (changedKey === null || changedKey === this.key) this.invalidate()
      })
    } catch (error) {
      this.listeners.delete(listener)
      throw error
    }

    if (this.getSnapshot() !== before) listener()

    return () => {
      this.listeners.delete(listener)
      if (this.listeners.size === 0) {
        this.subscription?.remove()
        this.subscription = undefined
      }
    }
  }

  invalidate(): void {
    for (const listener of this.listeners) listener()
  }
}

function valueSnapshot<T>(
  value: T | undefined,
  previous?: Snapshot<T>
): Snapshot<T> {
  if (previous !== undefined && Object.is(previous.value, value)) {
    return previous
  }
  return { value }
}

const readString: SnapshotReader<string> = (kv, key, previous) =>
  valueSnapshot(kv.getString(key), previous)
const readNumber: SnapshotReader<number> = (kv, key, previous) =>
  valueSnapshot(kv.getNumber(key), previous)
const readBoolean: SnapshotReader<boolean> = (kv, key, previous) =>
  valueSnapshot(kv.getBoolean(key), previous)
const readBuffer: SnapshotReader<ArrayBuffer> = (kv, key) =>
  valueSnapshot(kv.getBuffer(key))

interface JSONSnapshot<T> extends Snapshot<T> {
  json: string | undefined
}

const readJSON = <T>(
  kv: KV,
  key: string,
  previous?: Snapshot<T>
): JSONSnapshot<T> => {
  const json = kv[INTERNAL_GET_JSON_TEXT](key)
  const previousJSON = previous as JSONSnapshot<T> | undefined
  if (previousJSON !== undefined && previousJSON.json === json) {
    return previousJSON
  }
  return {
    json,
    value: json === undefined ? undefined : (JSON.parse(json) as T),
  }
}

const sameValue = <T>(previous: Snapshot<T>, next: Snapshot<T>): boolean =>
  Object.is(previous.value, next.value)

const sameJSON = <T>(
  previous: Snapshot<T>,
  next: Snapshot<T>
): boolean =>
  (previous as JSONSnapshot<T>).json === (next as JSONSnapshot<T>).json

function sameBuffer(
  previous: Snapshot<ArrayBuffer>,
  next: Snapshot<ArrayBuffer>
): boolean {
  const a = previous.value
  const b = next.value
  if (a === b) return true
  if (a === undefined || b === undefined || a.byteLength !== b.byteLength) {
    return false
  }
  const left = new Uint8Array(a)
  const right = new Uint8Array(b)
  for (let i = 0; i < left.length; i++) {
    if (left[i] !== right[i]) return false
  }
  return true
}

function useSnapshotSource<T>(
  key: string,
  kv: KV | undefined,
  read: SnapshotReader<T>,
  equals: SnapshotEquals<T>
): { source: KVSnapshotSource<T>; snapshot: Snapshot<T> } {
  const store = kv ?? getDefaultKV()
  const source = useMemo(
    () => new KVSnapshotSource(store, key, read, equals),
    [store, key, read, equals]
  )
  const snapshot = useSyncExternalStore(
    source.subscribe,
    source.getSnapshot,
    source.getSnapshot
  )
  return { source, snapshot }
}

function useKVValue<T>(
  key: string,
  kv: KV | undefined,
  read: SnapshotReader<T>,
  equals: SnapshotEquals<T>,
  write: (kv: KV, key: string, value: T) => void
): [T | undefined, (value: T | undefined) => void] {
  const store = kv ?? getDefaultKV()
  const { source, snapshot } = useSnapshotSource(key, store, read, equals)
  const set = useCallback(
    (next: T | undefined) => {
      if (next === undefined) store.delete(key)
      else write(store, key, next)
      source.invalidate()
    },
    [key, source, store, write]
  )

  return [snapshot.value, set]
}

const writeValue = <T extends string | number | boolean | ArrayBuffer>(
  store: KV,
  key: string,
  value: T
): void => store.set(key, value)

const writeJSON = <T>(store: KV, key: string, value: T): void =>
  store.setJSON(key, value)

export function useKVString(
  key: string,
  kv?: KV
): [string | undefined, (value: string | undefined) => void] {
  return useKVValue(key, kv, readString, sameValue, writeValue)
}

export function useKVNumber(
  key: string,
  kv?: KV
): [number | undefined, (value: number | undefined) => void] {
  return useKVValue(key, kv, readNumber, sameValue, writeValue)
}

export function useKVBoolean(
  key: string,
  kv?: KV
): [boolean | undefined, (value: boolean | undefined) => void] {
  return useKVValue(key, kv, readBoolean, sameValue, writeValue)
}

export function useKVBuffer(
  key: string,
  kv?: KV
): [ArrayBuffer | undefined, (value: ArrayBuffer | undefined) => void] {
  return useKVValue(key, kv, readBuffer, sameBuffer, writeValue)
}

export function useKVJSON<T = unknown>(
  key: string,
  kv?: KV
): [T | undefined, (value: T | undefined) => void] {
  return useKVValue<T>(key, kv, readJSON, sameJSON, writeJSON)
}

interface SelectionInstance<S> {
  hasValue: boolean
  value: S | undefined
}

function createSelectionGetter<T, S>(
  source: KVSnapshotSource<T>,
  selector: (value: T | undefined) => S,
  equals: (a: S, b: S) => boolean,
  instance: SelectionInstance<S>
): () => S {
  let hasMemo = false
  let previousSnapshot: Snapshot<T>
  let previousSelection: S

  return (): S => {
    const snapshot = source.getSnapshot()
    if (!hasMemo) {
      hasMemo = true
      previousSnapshot = snapshot
      const nextSelection = selector(snapshot.value)
      if (instance.hasValue && equals(instance.value as S, nextSelection)) {
        previousSelection = instance.value as S
      } else {
        previousSelection = nextSelection
      }
      return previousSelection
    }
    if (Object.is(previousSnapshot, snapshot)) return previousSelection

    const nextSelection = selector(snapshot.value)
    previousSnapshot = snapshot
    if (equals(previousSelection, nextSelection)) return previousSelection
    previousSelection = nextSelection
    return nextSelection
  }
}

function useSnapshotSelector<T, S>(
  source: KVSnapshotSource<T>,
  selector: (value: T | undefined) => S,
  equals: (a: S, b: S) => boolean
): S {
  const instanceRef = useRef<SelectionInstance<S> | undefined>(undefined)
  if (instanceRef.current === undefined) {
    instanceRef.current = { hasValue: false, value: undefined }
  }
  const instance = instanceRef.current

  const getSelection = useMemo(
    () => createSelectionGetter(source, selector, equals, instance),
    [equals, instance, selector, source]
  )

  const selected = useSyncExternalStore(
    source.subscribe,
    getSelection,
    getSelection
  )
  useEffect(() => {
    instance.hasValue = true
    instance.value = selected
  }, [instance, selected])
  return selected
}

export function useKVSelector<T = unknown, S = T | undefined>(
  key: string,
  selector: (value: T | undefined) => S,
  kv?: KV,
  equals: (a: S, b: S) => boolean = Object.is
): S {
  const store = kv ?? getDefaultKV()
  const source = useMemo(
    () => new KVSnapshotSource<T>(store, key, readJSON, sameJSON),
    [store, key]
  )
  return useSnapshotSelector(source, selector, equals)
}

/** @internal Test seam for snapshot identity and selector memoization. */
export const __hookInternals = {
  KVSnapshotSource,
  createSelectionGetter,
  readBuffer,
  readJSON,
  sameBuffer,
  sameJSON,
}
