jest.mock(
  'react-native-nitro-modules',
  () => require('./mockNitro').mockNitroModule
)

import { resetStores } from './mockNitro'
import { createKV } from '../src/kv'

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
