import type { MMKV } from 'react-native-mmkv'
import type { KV } from 'react-native-scc-storage'
import type { Rng } from './stats'

export type Scenario = 'small' | 'large'

type FamilyName =
  | 'str16'
  | 'str256'
  | 'str4k'
  | 'num'
  | 'bool'
  | 'json'
  | 'missing'

export interface Family {
  keys: string[]
  order: Int32Array
}

export const ORDER_MASK = 4095
export const VARIANT_MASK = 7

export type Dataset = Record<FamilyName, Family> & {
  scenario: Scenario
  totalKeys: number
  p16: string[]
  p256: string[]
  p4k: string[]
  docs: Record<string, unknown>[]
  docTexts: string[]
}

const FAMILY_SIZES: Record<Scenario, Record<FamilyName, number>> = {
  small: {
    str16: 100,
    str256: 100,
    str4k: 32,
    num: 100,
    bool: 100,
    json: 50,
    missing: 100,
  },
  large: {
    str16: 10_000,
    str256: 0,
    str4k: 0,
    num: 10_000,
    bool: 0,
    json: 0,
    missing: 10_000,
  },
}

const ALPHABET = 'abcdefghijklmnopqrstuvwxyz0123456789'

export function makePayloads(bytes: number): string[] {
  return Array.from({ length: VARIANT_MASK + 1 }, (_, variant) => {
    let out = ''
    for (let index = 0; index < bytes; index++) {
      out += ALPHABET[(index + variant * 7 + 1) % ALPHABET.length]
    }
    return out
  })
}

function makeDoc(variant: number): Record<string, unknown> {
  return {
    id: `user-${variant}`,
    revision: variant,
    settings: {
      theme: variant % 2 === 0 ? 'dark' : 'light',
      locale: 'en-US',
      notifications: { push: true, email: variant % 3 === 0, digest: 'weekly' },
    },
    recent: Array.from({ length: 10 }, (_, index) => ({
      id: index * 17 + variant,
      title: `Recently viewed item ${index} ${ALPHABET.slice(index, index + 16)}`,
      score: (index * 31 + variant) % 97,
      pinned: index % 4 === 0,
    })),
  }
}

function makeFamily(name: FamilyName, count: number, rng: Rng): Family {
  const keys = Array.from(
    { length: count },
    (_, index) => `${name}.item.${String(index).padStart(5, '0')}`
  )
  const order = new Int32Array(ORDER_MASK + 1)
  for (let index = 0; index < order.length && count > 0; index++) {
    order[index] = Math.floor(rng() * count)
  }
  return { keys, order }
}

export function makeDataset(scenario: Scenario, rng: Rng): Dataset {
  const sizes = FAMILY_SIZES[scenario]
  const docs = Array.from({ length: VARIANT_MASK + 1 }, (_, variant) =>
    makeDoc(variant)
  )
  const families = Object.fromEntries(
    (Object.keys(sizes) as FamilyName[]).map((name) => [
      name,
      makeFamily(name, sizes[name], rng),
    ])
  ) as Record<FamilyName, Family>
  return {
    ...families,
    scenario,
    totalKeys:
      sizes.str16 +
      sizes.str256 +
      sizes.str4k +
      sizes.num +
      sizes.bool +
      sizes.json,
    p16: makePayloads(16),
    p256: makePayloads(256),
    p4k: makePayloads(4096),
    docs,
    docTexts: docs.map((doc) => JSON.stringify(doc)),
  }
}

function stringEntries(keys: string[], value: string): Record<string, string> {
  return Object.fromEntries(keys.map((key) => [key, value]))
}

export function seedScc(kv: KV, data: Dataset): void {
  kv.setMany(stringEntries(data.str16.keys, data.p16[0]!))
  kv.setMany(stringEntries(data.str256.keys, data.p256[0]!))
  kv.setMany(stringEntries(data.str4k.keys, data.p4k[0]!))
  data.num.keys.forEach((key, index) => kv.set(key, index + 0.5))
  data.bool.keys.forEach((key, index) => kv.set(key, index % 2 === 0))
  for (const key of data.json.keys) kv.setJSON(key, data.docs[0])
}

export function seedMmkv(mmkv: MMKV, data: Dataset): void {
  for (const key of data.str16.keys) mmkv.set(key, data.p16[0]!)
  for (const key of data.str256.keys) mmkv.set(key, data.p256[0]!)
  for (const key of data.str4k.keys) mmkv.set(key, data.p4k[0]!)
  data.num.keys.forEach((key, index) => mmkv.set(key, index + 0.5))
  data.bool.keys.forEach((key, index) => mmkv.set(key, index % 2 === 0))
  for (const key of data.json.keys) mmkv.set(key, data.docTexts[0]!)
}

export function verifySeed(kv: KV, mmkv: MMKV, data: Dataset): void {
  const fail = (what: string) => {
    throw new Error(`benchmark seed verification failed: ${what}`)
  }
  const same = (scc: unknown, other: unknown, expected?: unknown) =>
    scc !== undefined && scc === other && (expected === undefined || scc === expected)
  const sample = (family: Family) =>
    family.keys.filter((_, index) => index % 97 === 0)

  if (kv.size !== data.totalKeys) fail(`scc size ${kv.size}`)
  if (mmkv.length !== data.totalKeys) fail(`mmkv size ${mmkv.length}`)
  for (const key of sample(data.str16)) {
    if (!same(kv.getString(key), mmkv.getString(key), data.p16[0])) fail(key)
  }
  for (const key of sample(data.str4k)) {
    if (!same(kv.getString(key), mmkv.getString(key), data.p4k[0])) fail(key)
  }
  for (const key of sample(data.num)) {
    if (!same(kv.getNumber(key), mmkv.getNumber(key))) fail(key)
  }
  for (const key of sample(data.json)) {
    const scc = JSON.stringify(kv.getJSON<unknown>(key))
    if (!same(scc, mmkv.getString(key), data.docTexts[0])) fail(key)
  }
  for (const key of sample(data.missing)) {
    if (kv.contains(key) || mmkv.contains(key)) fail(key)
  }
}
