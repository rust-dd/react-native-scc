jest.mock(
  'react-native-nitro-modules',
  () => require('./mockNitro').mockNitroModule
)

import { resetStores } from './mockNitro'
import { __hookInternals } from '../src/hooks'
import { createKV } from '../src/kv'

const flushMicrotasks = () =>
  new Promise<void>((resolve) => setTimeout(resolve, 0))

beforeEach(() => resetStores())

test('JSON snapshots stay referentially stable until serialized data changes', async () => {
  const kv = createKV({ id: 'hook-json-snapshot' })
  kv.setJSON('document', { selected: 1, other: 1 })
  const source = new __hookInternals.KVSnapshotSource(
    kv,
    'document',
    __hookInternals.readJSON,
    __hookInternals.sameJSON
  )

  const initial = source.getSnapshot()
  expect(source.getSnapshot()).toBe(initial)

  let changes = 0
  const unsubscribe = source.subscribe(() => {
    changes += 1
  })
  kv.setJSON('document', { selected: 1, other: 1 })
  await flushMicrotasks()

  expect(changes).toBe(1)
  expect(source.getSnapshot()).toBe(initial)

  kv.setJSON('document', { selected: 1, other: 2 })
  await flushMicrotasks()
  expect(source.getSnapshot()).not.toBe(initial)
  unsubscribe()
})

test('snapshots observe cross-handle writes before async notifications arrive', () => {
  const kv = createKV({ id: 'hook-cross-handle-snapshot' })
  const otherHandle = createKV({ id: 'hook-cross-handle-snapshot' })
  kv.set('value', 'before')
  const source = new __hookInternals.KVSnapshotSource(
    kv,
    'value',
    (store, key) => ({ value: store.getString(key) }),
    (previous, next) => previous.value === next.value
  )

  const initial = source.getSnapshot()
  otherHandle.set('value', 'after')

  expect(source.getSnapshot()).not.toBe(initial)
  expect(source.getSnapshot().value).toBe('after')
})

test('selector snapshots preserve equal selections and recompute new selectors immediately', async () => {
  const kv = createKV({ id: 'hook-selector-snapshot' })
  kv.setJSON('document', { selected: { id: 1 }, other: 1 })
  const source = new __hookInternals.KVSnapshotSource<{
    selected: { id: number }
    other: number
  }>(
    kv,
    'document',
    __hookInternals.readJSON,
    __hookInternals.sameJSON
  )
  const unsubscribe = source.subscribe(() => {})
  const instance = {
    hasValue: false,
    value: undefined as { id: number } | undefined,
  }
  const getSelected = __hookInternals.createSelectionGetter(
    source,
    (value) => value?.selected ?? { id: 0 },
    (a, b) => a.id === b.id,
    instance
  )
  const initialSelection = getSelected()
  instance.hasValue = true
  instance.value = initialSelection

  kv.setJSON('document', { selected: { id: 1 }, other: 2 })
  await flushMicrotasks()
  expect(getSelected()).toBe(initialSelection)

  const getOther = __hookInternals.createSelectionGetter(
    source,
    (value) => value?.other,
    Object.is,
    { hasValue: true, value: undefined }
  )
  expect(getOther()).toBe(2)
  unsubscribe()
})

test('buffer snapshots compare bytes instead of fresh ArrayBuffer identities', async () => {
  const kv = createKV({ id: 'hook-buffer-snapshot' })
  kv.set('buffer', new Uint8Array([1, 2, 3]).buffer)
  const source = new __hookInternals.KVSnapshotSource(
    kv,
    'buffer',
    __hookInternals.readBuffer,
    __hookInternals.sameBuffer
  )
  const initial = source.getSnapshot()
  const unsubscribe = source.subscribe(() => {})

  kv.set('buffer', new Uint8Array([1, 2, 3]).buffer)
  await flushMicrotasks()
  expect(source.getSnapshot()).toBe(initial)

  kv.set('buffer', new Uint8Array([1, 2, 4]).buffer)
  await flushMicrotasks()
  expect(source.getSnapshot()).not.toBe(initial)
  unsubscribe()
})
