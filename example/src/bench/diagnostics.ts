import { type Dataset, ORDER_MASK, VARIANT_MASK } from './fixtures'
import {
  type Harness,
  nextFrame,
  openScc,
  prepareStores,
  settleScc,
} from './harness'
import type { Diagnostic } from './report'
import { median } from './stats'

const REPEATS = 5

interface RawInstance {
  getString(key: string): string | undefined
}

function diagnostic(
  entry: Omit<Diagnostic, 'median'>
): Diagnostic {
  return { ...entry, median: median(entry.values) }
}

function wrapperOverhead(harness: Harness, data: Dataset): Diagnostic[] {
  const kv = harness.stores.scc
  const raw = (kv as unknown as { native: RawInstance }).native
  const { keys, order } = data.str16
  const run = (via: 'kv' | 'native', count: number) => {
    let sum = 0
    for (let index = 0; index < count; index++) {
      const key = keys[order[index & ORDER_MASK]!]!
      const value = via === 'kv' ? kv.getString(key) : raw.getString(key)
      sum += value?.length ?? 0
    }
    return sum
  }
  const samples: Record<'kv' | 'native', number[]> = { kv: [], native: [] }
  for (let round = 0; round < 12; round++) {
    const order = round % 2 === 0 ? (['kv', 'native'] as const) : (['native', 'kv'] as const)
    for (const via of order) {
      run(via, 200)
      const startedAt = performance.now()
      run(via, 2000)
      samples[via].push(((performance.now() - startedAt) * 1e6) / 2000)
    }
  }
  return [
    diagnostic({
      id: 'scc_kv_get_str16',
      label: 'getString · 16 B via KV',
      detail: 'public KV wrapper (what apps call)',
      unit: 'ns/op',
      values: samples.kv,
    }),
    diagnostic({
      id: 'scc_native_get_str16',
      label: 'getString · 16 B via raw Nitro',
      detail: 'same call without the JS KV wrapper',
      unit: 'ns/op',
      values: samples.native,
    }),
  ]
}

function reopenTimes(harness: Harness, data: Dataset): number[] {
  const values: number[] = []
  for (let round = 0; round < REPEATS; round++) {
    harness.stores.scc.close()
    const startedAt = performance.now()
    harness.stores.scc = openScc(false)
    values.push(performance.now() - startedAt)
    if (harness.stores.scc.size !== data.totalKeys) {
      throw new Error(`reopen lost data: ${harness.stores.scc.size}/${data.totalKeys}`)
    }
  }
  return values
}

function durableWrites(
  harness: Harness,
  data: Dataset
): Record<'total' | 'flush', number[]> {
  const scc = harness.stores.scc
  const { keys, order } = data.str256
  const out: Record<'total' | 'flush', number[]> = { total: [], flush: [] }
  for (let round = 0; round < REPEATS; round++) {
    settleScc(scc)
    const startedAt = performance.now()
    for (let index = 0; index < 1000; index++) {
      const payload = data.p256[(index + round) & VARIANT_MASK]!
      scc.set(keys[order[index & ORDER_MASK]!]!, payload)
    }
    const flushStartedAt = performance.now()
    scc.flush()
    const endedAt = performance.now()
    out.total.push(endedAt - startedAt)
    out.flush.push(endedAt - flushStartedAt)
  }
  return out
}

function reopenDiagnostic(id: string, harness: Harness, data: Dataset): Diagnostic {
  return diagnostic({
    id,
    label: `reopen · ${data.totalKeys}-key store`,
    detail: 'close() then createKV(): WAL replay into RAM',
    unit: 'ms',
    values: reopenTimes(harness, data),
  })
}

export const DIAGNOSTIC_STEPS = 4

export async function runDiagnostics(
  harness: Harness,
  datasets: Record<'small' | 'large', Dataset>,
  step: (label: string) => void
): Promise<Diagnostic[]> {
  const out: Diagnostic[] = []
  step('diagnostics · KV wrapper vs raw Nitro')
  await prepareStores(harness, datasets.small)
  out.push(...wrapperOverhead(harness, datasets.small))
  await nextFrame()

  step('diagnostics · durable writes')
  const durable = durableWrites(harness, datasets.small)
  out.push(
    diagnostic({
      id: 'scc_durable_1000x256',
      label: '1000 × set 256 B + flush()',
      detail: 'time until every write is fsynced',
      unit: 'ms',
      values: durable.total,
    }),
    diagnostic({
      id: 'scc_flush_after_1000x256',
      label: 'flush() after 1000 writes',
      detail: 'fsync barrier alone',
      unit: 'ms',
      values: durable.flush,
    })
  )
  await nextFrame()

  step('diagnostics · reopen small store')
  out.push(reopenDiagnostic('scc_open_small', harness, datasets.small))
  await nextFrame()

  step('diagnostics · reopen large store')
  await prepareStores(harness, datasets.large)
  out.push(reopenDiagnostic('scc_open_large', harness, datasets.large))
  return out
}
