type Listener = (key?: string) => void

class FakeStore {
  map = new Map<string, { tag: number; value: unknown; expiresAt?: number }>()
  listeners = new Map<number, Listener>()
  nextListenerId = 1

  notify(key: string | undefined) {
    for (const listener of this.listeners.values()) {
      queueMicrotask(() => listener(key))
    }
  }
}

const stores = new Map<string, FakeStore>()

function mockGetStore(id: string): FakeStore {
  let store = stores.get(id)
  if (store === undefined) {
    store = new FakeStore()
    stores.set(id, store)
  }
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
    store.map.set(key, {
      tag,
      value,
      expiresAt: ttlMs !== undefined ? Date.now() + ttlMs : undefined,
    })
    store.notify(key)
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
    remove: (k: string) => {
      const existed = store.map.delete(k)
      if (existed) store.notify(k)
      return existed
    },
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
    close: () => {},
    addListener: (listener: Listener) => {
      const id = store.nextListenerId++
      store.listeners.set(id, listener)
      return id
    },
    removeListener: (id: number) => store.listeners.delete(id),
    setStringTtl: (k: string, v: string, ttl: number) => set(k, 0, v, ttl),
    setNumberTtl: (k: string, v: number, ttl: number) => set(k, 1, v, ttl),
    setBooleanTtl: (k: string, v: boolean, ttl: number) => set(k, 2, v, ttl),
    setBufferTtl: (k: string, v: ArrayBuffer, ttl: number) => set(k, 3, v, ttl),
    setJsonTtl: (k: string, v: string, ttl: number) => set(k, 4, v, ttl),
    setManyString: (keys: string[], values: string[]) => {
      keys.forEach((k, i) => set(k, 0, values[i]))
    },
    getManyString: (keys: string[]) => keys.map((k) => get(k, 0) ?? null),
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
          if (store.map.delete(key)) store.notify(key)
          continue
        }
        const tag = raw[off]!
        off += 1
        const valLen = view.getUint32(off, true)
        off += 4
        const valBytes = raw.subarray(off, off + valLen)
        off += valLen
        let value: unknown
        if (tag === 0 || tag === 4) value = dec.decode(valBytes)
        else if (tag === 1)
          value = new DataView(
            valBytes.buffer,
            valBytes.byteOffset,
            valBytes.byteLength
          ).getFloat64(0, true)
        else if (tag === 2) value = valBytes[0] === 1
        else value = valBytes.slice().buffer
        set(key, tag, value)
      }
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
        open: (_dir: string, id: string) => mockMakeInstance(mockGetStore(id)),
        inMemory: (id: string) => mockMakeInstance(mockGetStore(`mem:${id}`)),
      }
    },
  },
}
