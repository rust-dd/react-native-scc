import { createKV, type KVSubscription } from 'react-native-scc-storage'
import { createSccStorage } from 'react-native-scc-storage/redux'
import { kv, kvCrossHandle } from './storage'

export interface SelfTestResult {
  name: string
  ok: boolean
  detail: string
}

export type SelfTestUpdate = (results: SelfTestResult[]) => void

let recordedLaunches: number | undefined
let ephemeralStoreSequence = 0

function getLaunchCount(): number {
  if (recordedLaunches !== undefined) return recordedLaunches
  recordedLaunches = (kv.getNumber('launches') ?? 0) + 1
  kv.set('launches', recordedLaunches)
  return recordedLaunches
}

async function waitForCrossHandleChange(): Promise<string | null> {
  let subscription: KVSubscription | undefined
  let timer: ReturnType<typeof setTimeout> | undefined

  try {
    const event = new Promise<string | null>((resolve, reject) => {
      timer = setTimeout(() => reject(new Error('listener timeout')), 2000)
      subscription = kv.addOnValueChangedListener((key) => {
        if (key === 'cross_key') resolve(key)
      })
    })
    kvCrossHandle.set('cross_key', 'from-second-handle')
    return await event
  } finally {
    if (timer !== undefined) clearTimeout(timer)
    subscription?.remove()
  }
}

async function waitForSelectedChange(): Promise<string | undefined> {
  let subscription: KVSubscription | undefined
  let timer: ReturnType<typeof setTimeout> | undefined

  kv.setJSON('obs_settings', { theme: 'dark', volume: 1 })
  try {
    const event = new Promise<string | undefined>((resolve, reject) => {
      timer = setTimeout(() => reject(new Error('observe timeout')), 2000)
      subscription = kv.observeJSON<{ theme: string }, string | undefined>(
        'obs_settings',
        (settings) => settings?.theme,
        (theme) => {
          if (theme === 'light') resolve(theme)
        }
      )
    })
    kv.setJSON('obs_settings', { theme: 'light', volume: 1 })
    return await event
  } finally {
    if (timer !== undefined) clearTimeout(timer)
    subscription?.remove()
  }
}

export async function runSelfTest(
  onUpdate?: SelfTestUpdate
): Promise<SelfTestResult[]> {
  const results: SelfTestResult[] = []
  const check = (name: string, ok: boolean, detail = '') => {
    results.push({ name, ok, detail })
    onUpdate?.([...results])
  }

  const launches = getLaunchCount()
  console.log(`SCC_LAUNCHES=${launches}`)
  check('persistence: launch counter', launches >= 1, `launch #${launches}`)

  kv.set('str', 'hello')
  kv.set('num', 42.5)
  kv.set('bool', true)
  kv.setJSON('json', { nested: [1, 2, 3] })
  const buffer = new Uint8Array([9, 8, 7]).buffer
  kv.set('buf', buffer)

  check('sync string', kv.getString('str') === 'hello')
  check('sync number', kv.getNumber('num') === 42.5)
  check('sync boolean', kv.getBoolean('bool') === true)
  check('sync json', kv.getJSON<{ nested: number[] }>('json')?.nested[2] === 3)
  const roundTripBuffer = kv.getBuffer('buf')
  check(
    'sync buffer',
    roundTripBuffer !== undefined && new Uint8Array(roundTripBuffer)[1] === 8
  )
  check('type mismatch is undefined', kv.getString('num') === undefined)
  check(
    'contains / delete',
    kv.contains('str') && kv.delete('str') && !kv.contains('str')
  )
  check('keys', kv.getAllKeys().includes('num'))

  const syncBatch = {
    batch_a: 'alpha',
    batch_b: 'beta',
  }
  kv.setMany(syncBatch)
  const syncBatchValues = kv.getMany(['batch_b', 'batch_missing', 'batch_a'])
  check(
    'batch sync: order + missing',
    syncBatchValues[0] === 'beta' &&
      syncBatchValues[1] === undefined &&
      syncBatchValues[2] === 'alpha'
  )

  await kv.setManyAsync({ batch_async_a: 'one', batch_async_b: 'two' })
  const asyncBatchValues = await kv.getManyAsync([
    'batch_async_a',
    'batch_async_missing',
    'batch_async_b',
  ])
  check(
    'batch async: order + missing',
    asyncBatchValues[0] === 'one' &&
      asyncBatchValues[1] === undefined &&
      asyncBatchValues[2] === 'two'
  )

  await kv.setAsync('astr', 'async-hello')
  check('async roundtrip', (await kv.getStringAsync('astr')) === 'async-hello')
  check(
    'async missing is undefined',
    (await kv.getNumberAsync('does-not-exist')) === undefined
  )
  await kv.setJSONAsync('ajson', { ok: true })
  const asyncJson = await kv.getJSONAsync<{ ok: boolean }>('ajson')
  check('async json', asyncJson?.ok === true)
  await kv.flushAsync()
  check('async flush', true)

  try {
    check(
      'native listener (cross-handle)',
      (await waitForCrossHandleChange()) === 'cross_key'
    )
  } catch (error) {
    check('native listener (cross-handle)', false, String(error))
  }

  kv.set('ttl_key', 'temporary', { ttlMs: 400 })
  check('ttl: readable before expiry', kv.getString('ttl_key') === 'temporary')
  await new Promise((resolve) => setTimeout(resolve, 600))
  check('ttl: expired after deadline', kv.getString('ttl_key') === undefined)

  const vault = createKV({ id: 'vault', encryptionKey: 'example-passphrase' })
  const vaultLaunches = (vault.getNumber('launches') ?? 0) + 1
  vault.set('launches', vaultLaunches)
  vault.set('secret', 'classified')
  check(
    'encrypted vault roundtrip',
    vault.getString('secret') === 'classified',
    `test-only key · launch #${vaultLaunches}`
  )
  vault.flush()
  vault.close()

  kv.set('tx_drop', 'x')
  const transactionResult = kv.transaction((transaction) => {
    const next = (transaction.getNumber('tx_counter') ?? 0) + 1
    transaction.set('tx_counter', next)
    transaction.setJSON('tx_meta', { next })
    transaction.delete('tx_drop')
    return next
  })
  check(
    'transaction: atomic batch commit',
    kv.getNumber('tx_counter') === transactionResult &&
      kv.getJSON<{ next: number }>('tx_meta')?.next === transactionResult &&
      !kv.contains('tx_drop'),
    `commit #${transactionResult}`
  )

  const profile = kv.namespace('profile')
  profile.clearAll()
  profile.set('name', 'Ada')
  profile.setJSON('prefs', { theme: 'dark' })
  check(
    'namespace: scoped keys',
    profile.getString('name') === 'Ada' &&
      profile.size === 2 &&
      kv.getString('profile:name') === 'Ada' &&
      profile.getAllKeys().sort().join(',') === 'name,prefs'
  )
  check(
    'namespace: scoped clearAll',
    profile.clearAll() === 2 && !kv.contains('profile:name')
  )

  kv.deleteByPrefix('prefix_case:')
  kv.set('prefix_case:a', 'a')
  kv.set('prefix_case:b', 'b')
  const prefixedKeys = kv.getKeysByPrefix('prefix_case:').sort()
  const removedByPrefix = kv.deleteByPrefix('prefix_case:')
  check(
    'prefix helpers: list + delete',
    prefixedKeys.join(',') === 'prefix_case:a,prefix_case:b' &&
      removedByPrefix === 2 &&
      !kv.contains('prefix_case:a')
  )

  try {
    check(
      'observeJSON: selected change emits',
      (await waitForSelectedChange()) === 'light'
    )
  } catch (error) {
    check('observeJSON: selected change emits', false, String(error))
  }

  const reduxStorage = createSccStorage(kv.namespace('redux_contract'))
  await reduxStorage.setItem('state', '{"count":1}')
  const reduxValue = await reduxStorage.getItem('state')
  await reduxStorage.removeItem('state')
  check(
    'redux-persist adapter contract',
    reduxValue === '{"count":1}' &&
      (await reduxStorage.getItem('state')) === null
  )

  ephemeralStoreSequence += 1
  const cache = createKV({
    id: `evict-demo-${Date.now()}-${ephemeralStoreSequence}`,
    persistence: 'none',
    maxEntries: 8,
    ttlSweepIntervalMs: 50,
  })
  try {
    for (let index = 0; index < 32; index++) cache.set(`item_${index}`, index)
    await new Promise((resolve) => setTimeout(resolve, 400))
    check(
      'eviction: in-memory maxEntries cap',
      cache.size <= 8,
      `${cache.size}/32 kept`
    )
  } finally {
    cache.close()
  }

  kv.flush()
  return results
}
