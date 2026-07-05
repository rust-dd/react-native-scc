jest.mock(
  'react-native-nitro-modules',
  () => require('./mockNitro').mockNitroModule
)

import { resetStores } from './mockNitro'
import { createStore as createZustandStore } from 'zustand/vanilla'
import { persist, createJSONStorage } from 'zustand/middleware'
import { createStore as createJotaiStore } from 'jotai/vanilla'
import { createKV } from '../src/kv'
import { sccStateStorage } from '../src/adapters/zustand'
import { atomWithKV } from '../src/adapters/jotai'
import { createSccStorage } from '../src/adapters/redux'

const flushMicrotasks = () =>
  new Promise<void>((resolve) => setTimeout(resolve, 0))

beforeEach(() => resetStores())

test('zustand persist hydrates synchronously from kv', () => {
  const kv = createKV({ id: 'z' })
  const options = {
    name: 'bears',
    storage: createJSONStorage(() => sccStateStorage(kv)),
  }

  const first = createZustandStore(persist(() => ({ bears: 0 }), options))
  first.setState({ bears: 7 })
  expect(kv.getString('bears')).toContain('"bears":7')

  const second = createZustandStore(persist(() => ({ bears: 0 }), options))
  expect(second.getState().bears).toBe(7)
})

test('jotai atomWithKV persists and reacts to external writes', async () => {
  const kv = createKV({ id: 'j' })
  const counterAtom = atomWithKV('counter', 0, kv)
  const store = createJotaiStore()

  expect(store.get(counterAtom)).toBe(0)
  store.set(counterAtom, 5)
  expect(kv.getJSON<number>('counter')).toBe(5)

  // atomWithStorage only subscribes while the atom is mounted.
  const unsub = store.sub(counterAtom, () => {})
  const other = createKV({ id: 'j' })
  other.setJSON('counter', 9)
  await flushMicrotasks()
  expect(store.get(counterAtom)).toBe(9)
  unsub()
})

test('redux-persist engine contract', async () => {
  const kv = createKV({ id: 'r' })
  const engine = createSccStorage(kv)
  await engine.setItem('persist:root', '{"x":"1"}')
  await expect(engine.getItem('persist:root')).resolves.toBe('{"x":"1"}')
  await engine.removeItem('persist:root')
  await expect(engine.getItem('persist:root')).resolves.toBeNull()
})
