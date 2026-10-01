import type { MMKV } from 'react-native-mmkv'
import type { KV } from 'react-native-scc-storage'
import {
  type Dataset,
  type Family,
  ORDER_MASK,
  type Scenario,
  VARIANT_MASK,
} from './fixtures'

export type Lib = 'scc' | 'mmkv'

export interface Stores {
  scc: KV
  mmkv: MMKV
}

export type Body = (count: number, offset: number) => number

interface ScalarStore {
  getString(key: string): string | undefined
  getNumber(key: string): number | undefined
  getBoolean(key: string): boolean | undefined
  contains(key: string): boolean
  set(key: string, value: string | number | boolean): void
}

export interface Verification {
  keys: string[]
  kind: 'string' | 'number' | 'boolean' | 'json'
}

export interface ThroughputCase {
  id: string
  scenario: Scenario
  group: 'read' | 'write' | 'json' | 'batch' | 'listener'
  label: string
  detail: string
  chunkSize: number
  chunks: number
  opsPerCall: number
  listen?: boolean
  body(lib: Lib, stores: Stores, data: Dataset): Body
  verifies?: (data: Dataset) => Verification[]
}

export interface LatencyCase {
  id: string
  label: string
  detail: string
  samples: number
  op(lib: Lib, stores: Stores, data: Dataset): (position: number) => number
}

function keyAt(family: Family, position: number): string {
  return family.keys[family.order[position & ORDER_MASK]!]!
}

function readString(store: ScalarStore, family: Family): Body {
  return (count, offset) => {
    let sum = 0
    for (let index = 0; index < count; index++) {
      const value = store.getString(keyAt(family, offset + index))
      if (value !== undefined) sum += value.length
    }
    return sum
  }
}

function readNumber(store: ScalarStore, family: Family): Body {
  return (count, offset) => {
    let sum = 0
    for (let index = 0; index < count; index++) {
      sum += store.getNumber(keyAt(family, offset + index)) ?? 0
    }
    return sum
  }
}

function readBoolean(store: ScalarStore, family: Family): Body {
  return (count, offset) => {
    let sum = 0
    for (let index = 0; index < count; index++) {
      if (store.getBoolean(keyAt(family, offset + index)) === true) sum++
    }
    return sum
  }
}

function containsKey(store: ScalarStore, family: Family): Body {
  return (count, offset) => {
    let sum = 0
    for (let index = 0; index < count; index++) {
      if (store.contains(keyAt(family, offset + index))) sum++
    }
    return sum
  }
}

function writeString(store: ScalarStore, family: Family, payloads: string[]): Body {
  return (count, offset) => {
    for (let index = 0; index < count; index++) {
      const position = offset + index
      store.set(keyAt(family, position), payloads[position & VARIANT_MASK]!)
    }
    return count
  }
}

function writeNumber(store: ScalarStore, family: Family): Body {
  return (count, offset) => {
    for (let index = 0; index < count; index++) {
      const position = offset + index
      store.set(keyAt(family, position), position + 0.25)
    }
    return count
  }
}

function writeBoolean(store: ScalarStore, family: Family): Body {
  return (count, offset) => {
    for (let index = 0; index < count; index++) {
      const position = offset + index
      store.set(keyAt(family, position), (position & 1) === 0)
    }
    return count
  }
}

function scalar(lib: Lib, stores: Stores): ScalarStore {
  return lib === 'scc' ? stores.scc : stores.mmkv
}

type Doc = { revision: number }

function batchKeys(data: Dataset): string[] {
  return data.str16.keys.slice(0, 100)
}

function batchEntries(data: Dataset): Record<string, string>[] {
  const keys = batchKeys(data)
  return data.p16.map((payload) =>
    Object.fromEntries(keys.map((key) => [key, payload]))
  )
}

const RANDOM_HIT = 'random key · resident hit'
const OVERWRITE = 'random existing key · value differs from the stored one'

interface ScalarCaseSpec {
  id: string
  label: string
  chunkSize: number
  scenario?: Scenario
  detail?: string
  make: (store: ScalarStore, data: Dataset) => Body
}

function readCase(spec: ScalarCaseSpec): ThroughputCase {
  return {
    id: spec.id,
    scenario: spec.scenario ?? 'small',
    group: 'read',
    label: spec.label,
    detail: spec.detail ?? RANDOM_HIT,
    chunkSize: spec.chunkSize,
    chunks: 12,
    opsPerCall: 1,
    body: (lib, stores, data) => spec.make(scalar(lib, stores), data),
  }
}

function writeCase(
  spec: ScalarCaseSpec & { verify: (data: Dataset) => Verification }
): ThroughputCase {
  return {
    ...readCase({ ...spec, detail: spec.detail ?? OVERWRITE }),
    group: 'write',
    verifies: (data) => [spec.verify(data)],
  }
}

export const throughputCases: ThroughputCase[] = [
  readCase({
    id: 'get_str16',
    label: 'getString · 16 B',
    chunkSize: 2000,
    make: (store, data) => readString(store, data.str16),
  }),
  readCase({
    id: 'get_str256',
    label: 'getString · 256 B',
    chunkSize: 1000,
    make: (store, data) => readString(store, data.str256),
  }),
  readCase({
    id: 'get_str4k',
    label: 'getString · 4 KiB',
    chunkSize: 250,
    make: (store, data) => readString(store, data.str4k),
  }),
  readCase({
    id: 'get_num',
    label: 'getNumber',
    chunkSize: 2000,
    make: (store, data) => readNumber(store, data.num),
  }),
  readCase({
    id: 'get_bool',
    label: 'getBoolean',
    chunkSize: 2000,
    make: (store, data) => readBoolean(store, data.bool),
  }),
  readCase({
    id: 'get_miss',
    label: 'getString · missing key',
    detail: 'random absent key',
    chunkSize: 2000,
    make: (store, data) => readString(store, data.missing),
  }),
  readCase({
    id: 'contains',
    label: 'contains',
    chunkSize: 2000,
    make: (store, data) => containsKey(store, data.str16),
  }),
  writeCase({
    id: 'set_str16',
    label: 'set string · 16 B',
    chunkSize: 1000,
    make: (store, data) => writeString(store, data.str16, data.p16),
    verify: (data) => ({ keys: data.str16.keys, kind: 'string' }),
  }),
  writeCase({
    id: 'set_str256',
    label: 'set string · 256 B',
    chunkSize: 500,
    make: (store, data) => writeString(store, data.str256, data.p256),
    verify: (data) => ({ keys: data.str256.keys, kind: 'string' }),
  }),
  writeCase({
    id: 'set_str4k',
    label: 'set string · 4 KiB',
    chunkSize: 100,
    make: (store, data) => writeString(store, data.str4k, data.p4k),
    verify: (data) => ({ keys: data.str4k.keys, kind: 'string' }),
  }),
  writeCase({
    id: 'set_num',
    label: 'set number',
    chunkSize: 1000,
    make: (store, data) => writeNumber(store, data.num),
    verify: (data) => ({ keys: data.num.keys, kind: 'number' }),
  }),
  writeCase({
    id: 'set_bool',
    label: 'set boolean',
    chunkSize: 1000,
    make: (store, data) => writeBoolean(store, data.bool),
    verify: (data) => ({ keys: data.bool.keys, kind: 'boolean' }),
  }),
  {
    id: 'set_json',
    scenario: 'small',
    group: 'json',
    label: 'setJSON · ~1 KiB object',
    detail: 'SCC setJSON vs MMKV set(JSON.stringify) · stringify timed for both',
    chunkSize: 200,
    chunks: 12,
    opsPerCall: 1,
    body: (lib, { scc, mmkv }, data) => (count, offset) => {
      for (let index = 0; index < count; index++) {
        const position = offset + index
        const key = keyAt(data.json, position)
        const doc = data.docs[position & VARIANT_MASK]
        if (lib === 'scc') scc.setJSON(key, doc)
        else mmkv.set(key, JSON.stringify(doc))
      }
      return count
    },
    verifies: (data) => [{ keys: data.json.keys, kind: 'json' }],
  },
  {
    id: 'get_json',
    scenario: 'small',
    group: 'json',
    label: 'getJSON · ~1 KiB object',
    detail: 'SCC getJSON vs JSON.parse(MMKV getString) · parse timed for both',
    chunkSize: 500,
    chunks: 12,
    opsPerCall: 1,
    body: (lib, { scc, mmkv }, data) => (count, offset) => {
      let sum = 0
      for (let index = 0; index < count; index++) {
        const key = keyAt(data.json, offset + index)
        if (lib === 'scc') {
          sum += scc.getJSON<Doc>(key)?.revision ?? 0
        } else {
          const text = mmkv.getString(key)
          if (text !== undefined) sum += (JSON.parse(text) as Doc).revision
        }
      }
      return sum
    },
  },
  {
    id: 'get_many',
    scenario: 'small',
    group: 'batch',
    label: 'read 100 × 16 B',
    detail: 'SCC getMany (one call) vs 100 MMKV getString calls · per key',
    chunkSize: 50,
    chunks: 12,
    opsPerCall: 100,
    body: (lib, { scc, mmkv }, data) => {
      const keys = batchKeys(data)
      return (count) => {
        let sum = 0
        for (let call = 0; call < count; call++) {
          if (lib === 'scc') {
            for (const value of scc.getMany(keys)) sum += value?.length ?? 0
          } else {
            for (const key of keys) sum += mmkv.getString(key)?.length ?? 0
          }
        }
        return sum
      }
    },
  },
  {
    id: 'set_many',
    scenario: 'small',
    group: 'batch',
    label: 'write 100 × 16 B',
    detail: 'SCC setMany (one call) vs 100 MMKV set calls · per key',
    chunkSize: 20,
    chunks: 12,
    opsPerCall: 100,
    body: (lib, { scc, mmkv }, data) => {
      const keys = batchKeys(data)
      const entries = batchEntries(data)
      return (count, offset) => {
        for (let call = 0; call < count; call++) {
          const variant = (offset + call) & VARIANT_MASK
          if (lib === 'scc') {
            scc.setMany(entries[variant]!)
          } else {
            const payload = data.p16[variant]!
            for (const key of keys) mmkv.set(key, payload)
          }
        }
        return count
      }
    },
    verifies: (data) => [{ keys: batchKeys(data), kind: 'string' }],
  },
  {
    ...writeCase({
      id: 'set_str16_listener',
      label: 'set string · 16 B · 1 listener',
      detail: 'one change listener per store; both deliver asynchronously',
      chunkSize: 1000,
      make: (store, data) => writeString(store, data.str16, data.p16),
      verify: (data) => ({ keys: data.str16.keys, kind: 'string' }),
    }),
    group: 'listener',
    listen: true,
  },
  readCase({
    id: 'large_get_str16',
    scenario: 'large',
    label: 'getString · 16 B · 20k-key store',
    chunkSize: 2000,
    make: (store, data) => readString(store, data.str16),
  }),
  readCase({
    id: 'large_get_num',
    scenario: 'large',
    label: 'getNumber · 20k-key store',
    chunkSize: 2000,
    make: (store, data) => readNumber(store, data.num),
  }),
  readCase({
    id: 'large_get_miss',
    scenario: 'large',
    label: 'getString · missing · 20k-key store',
    detail: 'random absent key',
    chunkSize: 2000,
    make: (store, data) => readString(store, data.missing),
  }),
  writeCase({
    id: 'large_set_str16',
    scenario: 'large',
    label: 'set string · 16 B · 20k-key store',
    chunkSize: 1000,
    make: (store, data) => writeString(store, data.str16, data.p16),
    verify: (data) => ({ keys: data.str16.keys, kind: 'string' }),
  }),
]

export const latencyCases: LatencyCase[] = [
  {
    id: 'idle_set_str16',
    label: 'set string · 16 B after idle',
    detail: 'one call per JS task, idle event loop between calls',
    samples: 150,
    op: (lib, stores, data) => {
      const store = scalar(lib, stores)
      return (position) => {
        store.set(keyAt(data.str16, position), data.p16[position & VARIANT_MASK]!)
        return 1
      }
    },
  },
  {
    id: 'idle_get_str16',
    label: 'getString · 16 B after idle',
    detail: 'one call per JS task, idle event loop between calls',
    samples: 150,
    op: (lib, stores, data) => {
      const store = scalar(lib, stores)
      return (position) => store.getString(keyAt(data.str16, position))?.length ?? 0
    },
  },
  {
    id: 'idle_set_json',
    label: 'persist ~1 KiB JSON after idle',
    detail: 'zustand-persist pattern: stringify + one write per interaction',
    samples: 150,
    op: (lib, { scc, mmkv }, data) => (position) => {
      const key = keyAt(data.json, position)
      const doc = data.docs[position & VARIANT_MASK]
      if (lib === 'scc') scc.setJSON(key, doc)
      else mmkv.set(key, JSON.stringify(doc))
      return 1
    },
  },
]
