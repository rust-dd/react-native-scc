import type { MMKV } from 'react-native-mmkv'
import type { KV } from 'react-native-scc-storage'
import { createSccBenchmarkStore } from '../storage'
import type { Lib, Stores, Verification } from './cases'
import { type Dataset, seedMmkv, seedScc, verifySeed } from './fixtures'
import type { Rng } from './stats'

export interface Profile {
  name: string
  throughputTrials: number
  latencyTrials: number
  scale: number
}

export interface Harness {
  stores: Stores
  profile: Profile
  rng: Rng
}

export function scaled(value: number, profile: Profile, minimum: number): number {
  return Math.max(minimum, Math.round(value * profile.scale))
}

export function openScc(recreate: boolean): KV {
  return createSccBenchmarkStore(recreate)
}

// The second flush waits behind any compaction the first one triggered.
export function settleScc(scc: KV): void {
  scc.flush()
  scc.flush()
}

export function nextFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()))
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

export async function prepareStores(harness: Harness, data: Dataset): Promise<void> {
  harness.stores.scc.close()
  harness.stores.scc = openScc(true)
  harness.stores.mmkv.clearAll()
  seedScc(harness.stores.scc, data)
  seedMmkv(harness.stores.mmkv, data)
  settleScc(harness.stores.scc)
  verifySeed(harness.stores.scc, harness.stores.mmkv, data)
  await nextFrame()
}

function sccValue(kv: KV, key: string, kind: Verification['kind']): unknown {
  switch (kind) {
    case 'string':
      return kv.getString(key)
    case 'number':
      return kv.getNumber(key)
    case 'boolean':
      return kv.getBoolean(key)
    case 'json':
      return JSON.stringify(kv.getJSON<unknown>(key))
  }
}

function mmkvValue(mmkv: MMKV, key: string, kind: Verification['kind']): unknown {
  switch (kind) {
    case 'string':
    case 'json':
      return mmkv.getString(key)
    case 'number':
      return mmkv.getNumber(key)
    case 'boolean':
      return mmkv.getBoolean(key)
  }
}

export function verifyState(caseId: string, stores: Stores, checks: Verification[]): void {
  for (const { keys, kind } of checks) {
    for (const key of keys) {
      const scc = sccValue(stores.scc, key, kind)
      const mmkv = mmkvValue(stores.mmkv, key, kind)
      if (scc === undefined || scc !== mmkv) {
        throw new Error(`${caseId}: stores diverged at ${key} (${String(scc)} vs ${String(mmkv)})`)
      }
    }
  }
}

export interface ListenerProbe {
  drain(lib: Lib, expected: number): Promise<void>
  remove(): void
}

export function listenBoth(stores: Stores): ListenerProbe {
  const delivered: Record<Lib, number> = { scc: 0, mmkv: 0 }
  const scc = stores.scc.addOnValueChangedListener(() => {
    delivered.scc++
  })
  const mmkv = stores.mmkv.addOnValueChangedListener(() => {
    delivered.mmkv++
  })
  return {
    async drain(lib, expected) {
      const deadline = performance.now() + 10_000
      while (delivered[lib] < expected) {
        if (performance.now() > deadline) {
          throw new Error(`${lib} delivered ${delivered[lib]}/${expected} change events`)
        }
        await sleep(5)
      }
      delivered[lib] = 0
    },
    remove() {
      scc.remove()
      mmkv.remove()
    },
  }
}
