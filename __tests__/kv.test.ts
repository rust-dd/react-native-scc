jest.mock(
  'react-native-nitro-modules',
  () => require('./mockNitro').mockNitroModule
)

import { resetStores } from './mockNitro'
import { createKV, getDefaultKV } from '../src/kv'

const flushMicrotasks = () =>
  new Promise<void>((resolve) => setTimeout(resolve, 0))

beforeEach(() => resetStores())

test('sync roundtrip and type mismatch', () => {
  const kv = createKV({ id: 't1' })
  kv.set('s', 'x')
  kv.set('n', 1.5)
  kv.setJSON('j', { a: [1] })
  expect(kv.getString('s')).toBe('x')
  expect(kv.getNumber('n')).toBe(1.5)
  expect(kv.getJSON<{ a: number[] }>('j')?.a[0]).toBe(1)
  expect(kv.getString('n')).toBeUndefined()
  expect(kv.getAllKeys().sort()).toEqual(['j', 'n', 's'])
})

test('async variants resolve', async () => {
  const kv = createKV({ id: 't2' })
  await kv.setAsync('k', 'v')
  await expect(kv.getStringAsync('k')).resolves.toBe('v')
  await expect(kv.getNumberAsync('nope')).resolves.toBeUndefined()
})

test('batch setMany/getMany round-trips with null for missing', async () => {
  const kv = createKV({ id: 't4' })
  kv.setMany({ a: '1', b: '2', c: '3' })
  expect(kv.getMany(['a', 'missing', 'c'])).toEqual(['1', undefined, '3'])
  await kv.setManyAsync({ d: '4' })
  await expect(kv.getManyAsync(['d', 'a'])).resolves.toEqual(['4', '1'])
})

test('empty batches avoid native calls', async () => {
  const kv = createKV({ id: 'empty-batch' })
  const native = (
    kv as unknown as {
      native: {
        setManyString(): void
        getManyString(): Array<string | null>
        setManyStringAsync(): Promise<void>
        getManyStringAsync(): Promise<Array<string | null>>
      }
    }
  ).native
  const setSpy = jest.spyOn(native, 'setManyString')
  const getSpy = jest.spyOn(native, 'getManyString')
  const setAsyncSpy = jest.spyOn(native, 'setManyStringAsync')
  const getAsyncSpy = jest.spyOn(native, 'getManyStringAsync')

  kv.setMany({})
  expect(kv.getMany([])).toEqual([])
  await kv.setManyAsync({})
  await expect(kv.getManyAsync([])).resolves.toEqual([])

  expect(setSpy).not.toHaveBeenCalled()
  expect(getSpy).not.toHaveBeenCalled()
  expect(setAsyncSpy).not.toHaveBeenCalled()
  expect(getAsyncSpy).not.toHaveBeenCalled()
})

test('transaction stages reads and writes atomically', () => {
  const kv = createKV({ id: 'tx' })
  kv.set('counter', 1)

  const result = kv.transaction((tx) => {
    const next = (tx.getNumber('counter') ?? 0) + 1
    tx.set('counter', next)
    tx.setJSON('meta', { next })
    expect(tx.getNumber('counter')).toBe(2)
    return next
  })

  expect(result).toBe(2)
  expect(kv.getNumber('counter')).toBe(2)
  expect(kv.getJSON<{ next: number }>('meta')?.next).toBe(2)
})

test('transaction rejects async callbacks', () => {
  const kv = createKV({ id: 'tx-async' })

  expect(() =>
    kv.transaction(() => Promise.resolve(undefined) as never)
  ).toThrow(/synchronous/)
})

test('transaction applies delete + mixed writes atomically', () => {
  const kv = createKV({ id: 'txmix' })
  kv.set('drop', 'x')
  kv.set('n', 1)

  kv.transaction((tx) => {
    tx.set('n', (tx.getNumber('n') ?? 0) + 1)
    tx.setJSON('meta', { ok: true })
    tx.delete('drop')
  })

  expect(kv.getNumber('n')).toBe(2)
  expect(kv.getJSON<{ ok: boolean }>('meta')?.ok).toBe(true)
  expect(kv.contains('drop')).toBe(false)
})

test('namespaced transaction scopes committed keys', () => {
  const kv = createKV({ id: 'txns' })
  const user = kv.namespace('u:1')

  user.transaction((tx) => {
    tx.set('name', 'Ada')
  })

  expect(user.getString('name')).toBe('Ada')
  expect(kv.getString('u:1:name')).toBe('Ada')
})

test('transaction commits through a single native batch call', () => {
  const kv = createKV({ id: 'txsingle' })
  const native = (
    kv as unknown as { native: { applyBatch(packed: ArrayBuffer): void } }
  ).native
  const spy = jest.spyOn(native, 'applyBatch')

  kv.transaction((tx) => {
    tx.set('x', 1)
    tx.setJSON('y', { a: 1 })
    tx.delete('z')
  })

  expect(spy).toHaveBeenCalledTimes(1)
  spy.mockRestore()
})

test('rejected async transaction discards staged writes', () => {
  const kv = createKV({ id: 'txdiscard' })

  expect(() =>
    kv.transaction((tx) => {
      tx.set('staged', 'v')
      return Promise.resolve(undefined) as never
    })
  ).toThrow(/synchronous/)

  expect(kv.contains('staged')).toBe(false)
})

test('transaction getJSON returns a snapshot copy', () => {
  const kv = createKV({ id: 'txsnap' })

  kv.transaction((tx) => {
    tx.setJSON('doc', { items: [1] })
    const doc = tx.getJSON<{ items: number[] }>('doc')!
    doc.items.push(2)
    expect(tx.getJSON<{ items: number[] }>('doc')!.items).toEqual([1])
  })

  expect(kv.getJSON<{ items: number[] }>('doc')!.items).toEqual([1])
})

test('transaction copies staged buffers', () => {
  const kv = createKV({ id: 'txbuf' })
  const buf = new Uint8Array([1, 2, 3]).buffer

  kv.transaction((tx) => {
    tx.set('b', buf)
    new Uint8Array(buf)[0] = 99
  })

  expect(new Uint8Array(kv.getBuffer('b')!)[0]).toBe(1)
})

test('JSON writes reject values without a JSON representation', async () => {
  const kv = createKV({ id: 'invalid-json' })
  const native = (
    kv as unknown as { native: { applyBatch(packed: ArrayBuffer): void } }
  ).native
  const batchSpy = jest.spyOn(native, 'applyBatch')

  for (const value of [undefined, () => undefined, Symbol('value')]) {
    expect(() => kv.setJSON('sync', value)).toThrow(/JSON-serializable/)
    await expect(kv.setJSONAsync('async', value)).rejects.toThrow(
      /JSON-serializable/
    )
    expect(() =>
      kv.transaction((tx) => tx.setJSON('transaction', value))
    ).toThrow(/JSON-serializable/)
  }

  expect(batchSpy).not.toHaveBeenCalled()
  expect(kv.getAllKeys()).toEqual([])
})

test('createKV validates eviction options for in-memory stores', () => {
  expect(() =>
    createKV({ id: 'evbad', persistence: 'none', maxEntries: -1 })
  ).toThrow(/maxEntries/)
  expect(() =>
    createKV({ id: 'evok', persistence: 'none', maxEntries: 100 })
  ).not.toThrow()
})

test('namespace scopes keys and prefix operations', () => {
  const kv = createKV({ id: 'prefix' })
  const user = kv.namespace('user:1')
  user.set('name', 'Ada')
  user.setJSON('prefs', { theme: 'dark' })
  kv.set('user:2:name', 'Grace')

  expect(user.getString('name')).toBe('Ada')
  expect(kv.getKeysByPrefix('user:1:').sort()).toEqual([
    'user:1:name',
    'user:1:prefs',
  ])
  expect(user.getAllKeys().sort()).toEqual(['name', 'prefs'])
  expect(user.size).toBe(2)
  expect(user.clearAll()).toBe(2)
  expect(kv.getString('user:1:name')).toBeUndefined()
  expect(kv.getString('user:2:name')).toBe('Grace')
})

test('namespaced prefix deletion uses one atomic native batch', () => {
  const kv = createKV({ id: 'prefix-batch' })
  const namespace = kv.namespace('user')
  namespace.set('cache:a', 'a')
  namespace.set('cache:b', 'b')
  namespace.set('keep', 'keep')
  kv.set('other:cache:a', 'other')
  const native = (
    kv as unknown as {
      native: {
        applyBatch(packed: ArrayBuffer): void
        remove(key: string): boolean
      }
    }
  ).native
  const batchSpy = jest.spyOn(native, 'applyBatch')
  const removeSpy = jest.spyOn(native, 'remove')

  expect(namespace.deleteByPrefix('cache:')).toBe(2)

  expect(batchSpy).toHaveBeenCalledTimes(1)
  expect(removeSpy).not.toHaveBeenCalled()
  expect(namespace.getAllKeys()).toEqual(['keep'])
  expect(kv.getString('other:cache:a')).toBe('other')

  batchSpy.mockClear()
  expect(namespace.clearAll()).toBe(1)
  expect(batchSpy).toHaveBeenCalledTimes(1)
  expect(removeSpy).not.toHaveBeenCalled()
  expect(namespace.getAllKeys()).toEqual([])
  expect(kv.getString('other:cache:a')).toBe('other')

  batchSpy.mockClear()
  expect(namespace.deleteByPrefix('missing:')).toBe(0)
  expect(batchSpy).not.toHaveBeenCalled()
})

test('namespaced async key operations stay on native async paths', async () => {
  const kv = createKV({ id: 'namespace-async' })
  const namespace = kv.namespace('scope')
  namespace.set('a', 'a')
  namespace.set('b', 'b')
  kv.set('outside', 'outside')
  const native = (
    kv as unknown as {
      native: {
        getAllKeys(): string[]
        getAllKeysAsync(): Promise<string[]>
        remove(key: string): boolean
        removeAsync(key: string): Promise<boolean>
      }
    }
  ).native
  const getSyncSpy = jest.spyOn(native, 'getAllKeys')
  const getAsyncSpy = jest.spyOn(native, 'getAllKeysAsync')
  const removeSyncSpy = jest.spyOn(native, 'remove')
  const removeAsyncSpy = jest.spyOn(native, 'removeAsync')

  await expect(namespace.getAllKeysAsync()).resolves.toEqual(['a', 'b'])
  expect(getAsyncSpy).toHaveBeenCalledTimes(1)
  expect(getSyncSpy).not.toHaveBeenCalled()

  await namespace.clearAllAsync()
  expect(getAsyncSpy).toHaveBeenCalledTimes(2)
  expect(removeAsyncSpy).toHaveBeenCalledTimes(2)
  expect(removeSyncSpy).not.toHaveBeenCalled()
  expect(namespace.getAllKeys()).toEqual([])
  expect(kv.getString('outside')).toBe('outside')
})

test('observeJSON emits selected changes only', async () => {
  const kv = createKV({ id: 'observe' })
  kv.setJSON('settings', { theme: 'dark', volume: 1 })
  const events: Array<string | undefined> = []

  const sub = kv.observeJSON<{ theme: string; volume: number }, string | undefined>(
    'settings',
    (value) => value?.theme,
    (theme) => events.push(theme)
  )

  kv.setJSON('settings', { theme: 'dark', volume: 2 })
  kv.setJSON('settings', { theme: 'light', volume: 2 })
  await flushMicrotasks()
  sub.remove()
  kv.setJSON('settings', { theme: 'blue', volume: 2 })
  await flushMicrotasks()

  expect(events).toEqual(['dark', 'light'])
})

test('observeJSON captures a write made by its initial listener', async () => {
  const kv = createKV({ id: 'observe-initial-write' })
  kv.setJSON('value', 1)
  const events: number[] = []

  const sub = kv.observeJSON<number, number | undefined>(
    'value',
    (value) => value,
    (value) => {
      if (value !== undefined) events.push(value)
      if (value === 1) kv.setJSON('value', 2)
    }
  )
  await flushMicrotasks()
  sub.remove()

  expect(events).toEqual([1, 2])
})

test('ttl option expires values', async () => {
  const kv = createKV({ id: 't5', encryptionKey: 'secret' })
  kv.set('tmp', 'v', { ttlMs: 30 })
  kv.setJSON('jtmp', { a: 1 }, { ttlMs: 30 })
  expect(kv.getString('tmp')).toBe('v')
  expect(kv.getJSON<{ a: number }>('jtmp')?.a).toBe(1)
  expect(kv.contains('tmp')).toBe(true)
  expect(kv.getAllKeys().sort()).toEqual(['jtmp', 'tmp'])
  expect(kv.size).toBe(2)
  await new Promise((resolve) => setTimeout(resolve, 60))
  expect(kv.getString('tmp')).toBeUndefined()
  expect(kv.getJSON('jtmp')).toBeUndefined()
  expect(kv.contains('tmp')).toBe(false)
  expect(kv.getAllKeys()).toEqual([])
  expect(kv.size).toBe(0)
})

test('ttl option rejects invalid durations', () => {
  const kv = createKV({ id: 'bad-ttl' })

  expect(() => kv.set('nan', 'v', { ttlMs: Number.NaN })).toThrow(/ttlMs/)
  expect(() => kv.setJSON('inf', { a: 1 }, { ttlMs: Infinity })).toThrow(
    /ttlMs/
  )
  expect(() => kv.set('zero', 'v', { ttlMs: 0 })).toThrow(/ttlMs/)
  expect(() =>
    kv.set('unsafe', 'v', { ttlMs: Number.MAX_SAFE_INTEGER + 1 })
  ).toThrow(/ttlMs/)
  expect(kv.getAllKeys()).toEqual([])
})

test('listener fires for cross-handle writes and clearAll', async () => {
  const a = createKV({ id: 't3' })
  const b = createKV({ id: 't3' })
  const events: Array<string | null> = []
  const sub = a.addOnValueChangedListener((key) => events.push(key))

  b.set('shared', 1)
  b.clearAll()
  await flushMicrotasks()
  expect(events).toEqual(['shared', null])

  sub.remove()
  b.set('after', 2)
  await flushMicrotasks()
  expect(events).toHaveLength(2)
})

test('duplicate listener registrations have independent subscriptions', async () => {
  const kv = createKV({ id: 'duplicate-listener' })
  const listener = jest.fn()
  const first = kv.addOnValueChangedListener(listener)
  const second = kv.addOnValueChangedListener(listener)

  first.remove()
  kv.set('key', 1)
  await flushMicrotasks()
  expect(listener).toHaveBeenCalledTimes(1)

  second.remove()
  kv.set('key', 2)
  await flushMicrotasks()
  expect(listener).toHaveBeenCalledTimes(1)
})

test('namespace cannot close its parent store', () => {
  const kv = createKV({ id: 'namespace-close' })
  const namespace = kv.namespace('scope')
  const native = (kv as unknown as { native: { close(): void } }).native
  const closeSpy = jest.spyOn(native, 'close')

  expect(() => namespace.close()).toThrow(/namespace/)
  expect(closeSpy).not.toHaveBeenCalled()
  expect(() => kv.set('still-open', true)).not.toThrow()
})

test('root close removes its native listener and is idempotent', () => {
  const kv = createKV({ id: 'root-close' })
  const native = (
    kv as unknown as {
      native: { close(): void; removeListener(id: number): boolean }
    }
  ).native
  const closeSpy = jest.spyOn(native, 'close')
  const removeListenerSpy = jest.spyOn(native, 'removeListener')
  kv.addOnValueChangedListener(() => {})

  kv.close()
  kv.close()

  expect(removeListenerSpy).toHaveBeenCalledTimes(1)
  expect(closeSpy).toHaveBeenCalledTimes(1)
  expect(() => kv.addOnValueChangedListener(() => {})).toThrow(/closed/)
})

test('closing the default store allows a fresh default instance', () => {
  const first = getDefaultKV()
  first.set('persisted', 'value')
  first.close()

  const reopened = getDefaultKV()
  expect(reopened).not.toBe(first)
  expect(reopened.getString('persisted')).toBe('value')
  reopened.close()
})
