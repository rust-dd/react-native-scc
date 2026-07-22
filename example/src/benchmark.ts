import { Platform } from 'react-native'
import type { MMKV } from 'react-native-mmkv'
import type { KV } from 'react-native-scc-storage'
import { createSccBenchmarkStore, kv, mmkvBench } from './storage'

const TRIALS = 4
const SCALAR_ITERATIONS = 100_000
const BATCH_ITERATIONS = 1_000
const BATCH_SIZE = 100
const payload64 = 'x'.repeat(64)
const payload16 = 'x'.repeat(16)
const batchKeys = Array.from(
  { length: BATCH_SIZE },
  (_, index) => `bm_${index}`
)
const batchEntries: Record<string, string> = Object.fromEntries(
  batchKeys.map((key) => [key, payload16])
)

let benchmarkSink = 0

export interface BenchmarkCase {
  id: string
  label: string
  detail: string
  scc: number
  mmkv: number
  operationsPerCall: number
  sccSamples: number[]
  mmkvSamples: number[]
}

export interface BenchmarkMetadata {
  methodologyVersion: 5
  createdAt: string
  platform: string
  platformVersion: string
  buildMode: 'development' | 'production'
  trials: number
  seededKeys: number
  scalarIterations: number
  batchIterations: number
  unit: 'nanoseconds-per-operation'
  measurement: 'synchronous-api-call-latency'
  sccDurability: 'relaxed-wal'
  drainPolicy: 'double-flush-idle-barrier-after-each-scc-sample'
  storeReset: 'physical-scc-recreate-per-trial'
}

export interface BenchmarkReport {
  metadata: BenchmarkMetadata
  results: BenchmarkCase[]
}

export interface BenchmarkProgress {
  completed: number
  total: number
  label: string
}

interface BenchmarkDefinition {
  id: string
  label: string
  detail: string
  iterations: number
  operationsPerIteration?: number
  scc: () => void
  mmkv: () => void
}

interface BenchmarkStores {
  scc: KV
  mmkv: MMKV
}

function yieldToUI(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()))
}

function measure(
  operation: () => void,
  iterations: number,
  operationsPerIteration = 1
): number {
  const warmupIterations = Math.min(
    1_000,
    Math.max(10, Math.floor(iterations / 100))
  )
  for (let index = 0; index < warmupIterations; index++) operation()

  const startedAt = performance.now()
  for (let index = 0; index < iterations; index++) operation()
  return (
    ((performance.now() - startedAt) * 1e6) /
    iterations /
    operationsPerIteration
  )
}

function median(values: number[]): number {
  const sorted = [...values].sort((left, right) => left - right)
  const middle = Math.floor(sorted.length / 2)
  return sorted.length % 2 === 0
    ? (sorted[middle - 1]! + sorted[middle]!) / 2
    : sorted[middle]!
}

function seedStores(stores: BenchmarkStores): void {
  stores.mmkv.clearAll()

  stores.scc.setMany(batchEntries)
  for (const key of batchKeys) stores.mmkv.set(key, payload16)

  stores.scc.set('b_s', payload64)
  stores.scc.set('b_s16', payload16)
  stores.scc.set('b_n', 42.5)
  stores.mmkv.set('b_s', payload64)
  stores.mmkv.set('b_s16', payload16)
  stores.mmkv.set('b_n', 42.5)
  stores.scc.flush()

  const sccSeed = stores.scc.getMany(batchKeys)
  const mmkvSeed = batchKeys.map((key) => stores.mmkv.getString(key))
  if (
    sccSeed.some((value) => value !== payload16) ||
    mmkvSeed.some((value) => value !== payload16) ||
    stores.scc.getString('b_s') !== payload64 ||
    stores.mmkv.getString('b_s') !== payload64 ||
    stores.scc.getString('b_s16') !== payload16 ||
    stores.mmkv.getString('b_s16') !== payload16 ||
    stores.scc.getNumber('b_n') !== 42.5 ||
    stores.mmkv.getNumber('b_n') !== 42.5
  ) {
    throw new Error('benchmark seed verification failed')
  }
}

function definitions(stores: BenchmarkStores): BenchmarkDefinition[] {
  return [
    {
      id: 'set100x16_perkey',
      label: 'setMany · 100 × 16 B',
      detail: 'one SCC batch call vs 100 MMKV scalar calls · per key',
      iterations: BATCH_ITERATIONS,
      operationsPerIteration: BATCH_SIZE,
      scc: () => stores.scc.setMany(batchEntries),
      mmkv: () => {
        for (const key of batchKeys) stores.mmkv.set(key, payload16)
      },
    },
    {
      id: 'get100x16_perkey',
      label: 'getMany · 100 × 16 B',
      detail: 'one SCC batch call vs 100 MMKV scalar calls · per key',
      iterations: BATCH_ITERATIONS,
      operationsPerIteration: BATCH_SIZE,
      scc: () => {
        benchmarkSink ^= stores.scc.getMany(batchKeys).length
      },
      mmkv: () => {
        benchmarkSink ^= batchKeys.map((key) => stores.mmkv.getString(key)).length
      },
    },
    {
      id: 'set_string64',
      label: 'set string · 64 B',
      detail: 'same-key overwrite',
      iterations: SCALAR_ITERATIONS,
      scc: () => stores.scc.set('b_s', payload64),
      mmkv: () => stores.mmkv.set('b_s', payload64),
    },
    {
      id: 'get_string64',
      label: 'get string · 64 B',
      detail: 'resident hit',
      iterations: SCALAR_ITERATIONS,
      scc: () => {
        benchmarkSink ^= stores.scc.getString('b_s')?.length ?? 0
      },
      mmkv: () => {
        benchmarkSink ^= stores.mmkv.getString('b_s')?.length ?? 0
      },
    },
    {
      id: 'set_string16',
      label: 'set string · 16 B',
      detail: 'same-key overwrite',
      iterations: SCALAR_ITERATIONS,
      scc: () => stores.scc.set('b_s16', payload16),
      mmkv: () => stores.mmkv.set('b_s16', payload16),
    },
    {
      id: 'get_string16',
      label: 'get string · 16 B',
      detail: 'resident hit',
      iterations: SCALAR_ITERATIONS,
      scc: () => {
        benchmarkSink ^= stores.scc.getString('b_s16')?.length ?? 0
      },
      mmkv: () => {
        benchmarkSink ^= stores.mmkv.getString('b_s16')?.length ?? 0
      },
    },
    {
      id: 'set_number',
      label: 'set number',
      detail: 'IEEE-754 double',
      iterations: SCALAR_ITERATIONS,
      scc: () => stores.scc.set('b_n', 42.5),
      mmkv: () => stores.mmkv.set('b_n', 42.5),
    },
    {
      id: 'get_number',
      label: 'get number',
      detail: 'resident hit',
      iterations: SCALAR_ITERATIONS,
      scc: () => {
        benchmarkSink ^= stores.scc.getNumber('b_n') === 42.5 ? 1 : 0
      },
      mmkv: () => {
        benchmarkSink ^= stores.mmkv.getNumber('b_n') === 42.5 ? 1 : 0
      },
    },
    {
      id: 'get_miss',
      label: 'get missing key',
      detail: 'typed lookup miss',
      iterations: SCALAR_ITERATIONS,
      scc: () => {
        benchmarkSink ^=
          stores.scc.getString('b_missing') === undefined ? 1 : 0
      },
      mmkv: () => {
        benchmarkSink ^=
          stores.mmkv.getString('b_missing') === undefined ? 1 : 0
      },
    },
  ]
}

export function getLastBenchmark(): BenchmarkReport | undefined {
  const stored: unknown = kv.getJSON<unknown>('last_bench')
  if (
    stored === null ||
    typeof stored !== 'object' ||
    !('metadata' in stored) ||
    !('results' in stored) ||
    stored.metadata === null ||
    typeof stored.metadata !== 'object' ||
    !('methodologyVersion' in stored.metadata) ||
    stored.metadata.methodologyVersion !== 5 ||
    !Array.isArray(stored.results)
  ) {
    return undefined
  }
  return stored as BenchmarkReport
}

async function collectBenchmark(
  stores: BenchmarkStores,
  onProgress?: (progress: BenchmarkProgress) => void
): Promise<BenchmarkReport> {
  const cases = definitions(stores)
  const total = cases.length * TRIALS
  let completed = 0
  const results: BenchmarkCase[] = []

  console.log(
    `SCC_BENCH_START trials=${TRIALS} seededKeys=${BATCH_SIZE + 3} scalarIterations=${SCALAR_ITERATIONS}`
  )

  for (let caseIndex = 0; caseIndex < cases.length; caseIndex++) {
    const benchmarkCase = cases[caseIndex]!
    const sccSamples: number[] = []
    const mmkvSamples: number[] = []

    for (let trial = 0; trial < TRIALS; trial++) {
      if (trial > 0 || caseIndex > 0) {
        stores.scc.close()
        stores.scc = createSccBenchmarkStore()
      }
      onProgress?.({
        completed,
        total,
        label: `${benchmarkCase.label} · seeding`,
      })
      await yieldToUI()
      seedStores(stores)
      await yieldToUI()
      onProgress?.({ completed, total, label: benchmarkCase.label })

      const operations = benchmarkCase.operationsPerIteration ?? 1
      const measureScc = () => {
        const elapsed = measure(
          benchmarkCase.scc,
          benchmarkCase.iterations,
          operations
        )
        stores.scc.flush()
        stores.scc.flush()
        return elapsed
      }
      const measureMmkv = () =>
        measure(benchmarkCase.mmkv, benchmarkCase.iterations, operations)

      if ((caseIndex + trial) % 2 === 0) {
        sccSamples.push(measureScc())
        mmkvSamples.push(measureMmkv())
      } else {
        mmkvSamples.push(measureMmkv())
        sccSamples.push(measureScc())
      }
      completed += 1
      onProgress?.({ completed, total, label: benchmarkCase.label })
    }

    const scc = median(sccSamples)
    const mmkv = median(mmkvSamples)
    if (
      !Number.isFinite(scc) ||
      !Number.isFinite(mmkv) ||
      scc <= 0 ||
      mmkv <= 0
    ) {
      throw new Error(`invalid timing for ${benchmarkCase.id}`)
    }
    console.log(
      `SCC_BENCH case=${benchmarkCase.id} scc=${scc.toFixed(0)}ns mmkv=${mmkv.toFixed(0)}ns`
    )
    results.push({
      id: benchmarkCase.id,
      label: benchmarkCase.label,
      detail: benchmarkCase.detail,
      scc,
      mmkv,
      operationsPerCall: benchmarkCase.operationsPerIteration ?? 1,
      sccSamples,
      mmkvSamples,
    })
  }

  const report: BenchmarkReport = {
    metadata: {
      methodologyVersion: 5,
      createdAt: new Date().toISOString(),
      platform: Platform.OS,
      platformVersion: String(Platform.Version),
      buildMode: __DEV__ ? 'development' : 'production',
      trials: TRIALS,
      seededKeys: BATCH_SIZE + 3,
      scalarIterations: SCALAR_ITERATIONS,
      batchIterations: BATCH_ITERATIONS,
      unit: 'nanoseconds-per-operation',
      measurement: 'synchronous-api-call-latency',
      sccDurability: 'relaxed-wal',
      drainPolicy: 'double-flush-idle-barrier-after-each-scc-sample',
      storeReset: 'physical-scc-recreate-per-trial',
    },
    results,
  }

  kv.setJSON('last_bench', report)
  kv.flush()
  console.log('SCC_BENCH_DONE')
  return report
}

export async function runBenchmark(
  onProgress?: (progress: BenchmarkProgress) => void
): Promise<BenchmarkReport> {
  await yieldToUI()
  const stores: BenchmarkStores = {
    scc: createSccBenchmarkStore(),
    mmkv: mmkvBench,
  }
  try {
    return await collectBenchmark(stores, onProgress)
  } finally {
    stores.scc.close()
  }
}
