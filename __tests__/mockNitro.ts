type Listener = { onChange: () => void; pending: (string | null)[] }

type Entry = { tag: number; value: unknown; expiresAt?: number }

class FakeStore {
  listeners = new Map<number, Listener>()
  nextListenerId = 1
  closed = false

  constructor(public map = new Map<string, Entry>()) {}

  notify(key: string | undefined) {
    for (const listener of this.listeners.values()) {
      listener.pending.push(key ?? null)
      if (listener.pending.length === 1) {
        queueMicrotask(() => {
          try {
            listener.onChange()
          } catch (error) {
            dispatchErrors.push(error)
          }
        })
      }
    }
  }
}

export const dispatchErrors: unknown[] = []

const stores = new Map<string, FakeStore>()

function mockGetStore(id: string, recreate = false): FakeStore {
  const live = stores.get(id)
  if (live !== undefined && !live.closed) {
    if (recreate) {
      throw new Error(
        `store '${id}' is already open; close it before reopening with recreate`
      )
    }
    return live
  }
  const store = new FakeStore(recreate ? undefined : live?.map)
  stores.set(id, store)
  return store
}

export function resetStores() {
  stores.clear()
}

function mockMakeInstance(store: FakeStore) {
  const isExpired = (entry: { expiresAt?: number }) =>
    entry.expiresAt !== undefined && Date.now() >= entry.expiresAt
  const get = (key: string, tag: number) => {
    const entry = store.map.get(key)
    if (entry === undefined || entry.tag !== tag) return undefined
    if (isExpired(entry)) return undefined
    return entry.value
  }
  const set = (key: string, tag: number, value: unknown, ttlMs?: number) => {
    if (store.closed) throw new Error('set failed: store is closed')
    store.map.set(key, {
      tag,
      value,
      expiresAt: ttlMs !== undefined ? Date.now() + ttlMs : undefined,
    })
    store.notify(key)
  }
  const removeLive = (key: string) => {
    const entry = store.map.get(key)
    const live = entry !== undefined && !isExpired(entry)
    store.map.delete(key)
    if (live) store.notify(key)
    return live
  }
  const instance = {
    setString: (k: string, v: string) => set(k, 0, v),
    setNumber: (k: string, v: number) => set(k, 1, v),
    setBoolean: (k: string, v: boolean) => set(k, 2, v),
    setBuffer: (k: string, v: ArrayBuffer) => set(k, 3, v),
    setJson: (k: string, v: string) => set(k, 4, v),
    getString: (k: string) => get(k, 0),
    getNumber: (k: string) => get(k, 1),
    getBoolean: (k: string) => get(k, 2),
    getBuffer: (k: string) => get(k, 3),
    getJson: (k: string) => get(k, 4),
    contains: (k: string) => {
      const entry = store.map.get(k)
      return entry !== undefined && !isExpired(entry)
    },
    remove: (k: string) => removeLive(k),
    getAllKeys: () =>
      [...store.map.entries()]
        .filter(([, entry]) => !isExpired(entry))
        .map(([key]) => key),
    clearAll: () => {
      store.map.clear()
      store.notify(undefined)
    },
    flush: () => {},
    size: () =>
      [...store.map.values()].filter((entry) => !isExpired(entry)).length,
    close: () => {
      store.closed = true
    },
    addListener: (onChange: () => void) => {
      const id = store.nextListenerId++
      store.listeners.set(id, { onChange, pending: [] })
      return id
    },
    removeListener: (id: number) => store.listeners.delete(id),
    takeChanges: (id: number) => {
      const listener = store.listeners.get(id)
      if (listener === undefined) return []
      const changes = listener.pending
      listener.pending = []
      return changes
    },
    setStringTtl: (k: string, v: string, ttl: number) => set(k, 0, v, ttl),
    setNumberTtl: (k: string, v: number, ttl: number) => set(k, 1, v, ttl),
    setBooleanTtl: (k: string, v: boolean, ttl: number) => set(k, 2, v, ttl),
    setBufferTtl: (k: string, v: ArrayBuffer, ttl: number) => set(k, 3, v, ttl),
    setJsonTtl: (k: string, v: string, ttl: number) => set(k, 4, v, ttl),
    setManyString: (keys: string[], values: string[]) => {
      keys.forEach((k, i) => set(k, 0, values[i]))
    },
    getManyString: (keys: string[]) => keys.map((k) => get(k, 0) ?? null),
    // Mirrors the native decoder's strictness so an encoder regression fails
    // in Jest instead of only throwing from native on device.
    applyBatch: (packed: ArrayBuffer) => {
      const view = new DataView(packed)
      const raw = new Uint8Array(packed)
      const dec = new TextDecoder()
      let off = 0
      const count = view.getUint32(off, true)
      off += 4
      for (let i = 0; i < count; i++) {
        const kind = raw[off]
        off += 1
        const keyLen = view.getUint32(off, true)
        off += 4
        const key = dec.decode(raw.subarray(off, off + keyLen))
        off += keyLen
        if (kind === 0) {
          removeLive(key)
          continue
        }
        if (kind !== 1) throw new Error(`applyBatch: unknown op kind ${kind}`)
        const tag = raw[off]!
        off += 1
        const valLen = view.getUint32(off, true)
        off += 4
        const valBytes = raw.subarray(off, off + valLen)
        off += valLen
        let value: unknown
        if (tag === 0 || tag === 4) value = dec.decode(valBytes)
        else if (tag === 1) {
          if (valBytes.byteLength !== 8)
            throw new Error('applyBatch: invalid number encoding')
          value = new DataView(
            valBytes.buffer,
            valBytes.byteOffset,
            valBytes.byteLength
          ).getFloat64(0, true)
        } else if (tag === 2) {
          if (valBytes.byteLength !== 1 || valBytes[0]! > 1)
            throw new Error('applyBatch: invalid bool encoding')
          value = valBytes[0] === 1
        } else if (tag === 3) value = valBytes.slice().buffer
        else throw new Error(`applyBatch: unknown value tag ${tag}`)
        set(key, tag, value)
      }
      if (off !== raw.byteLength)
        throw new Error('applyBatch: trailing bytes in batch buffer')
    },
  }
  const withAsync: Record<string, unknown> = { ...instance }
  for (const [name, fn] of Object.entries(instance)) {
    withAsync[`${name}Async`] = (...args: unknown[]) =>
      Promise.resolve((fn as (...a: unknown[]) => unknown)(...args))
  }
  return withAsync
}

export const mockNitroModule = {
  NitroModules: {
    createHybridObject: (name: string) => {
      if (name === 'SccKvPlatformContext') {
        return { getBaseDirectory: () => '/mock' }
      }
      return {
        open: (_dir: string, id: string, _strict: boolean, recreate: boolean) =>
          mockMakeInstance(mockGetStore(id, recreate)),
        inMemory: (id: string) => mockMakeInstance(mockGetStore(`mem:${id}`)),
      }
    },
  },
}
