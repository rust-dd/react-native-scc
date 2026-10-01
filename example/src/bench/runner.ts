import { Platform } from 'react-native'
import mmkvPackage from 'react-native-mmkv/package.json'
import nitroPackage from 'react-native-nitro-modules/package.json'
import sccPackage from 'react-native-scc-storage/package.json'
import { mmkvBench } from '../storage'
import {
  type LatencyCase,
  type Lib,
  latencyCases,
  type ThroughputCase,
  throughputCases,
} from './cases'
import { DIAGNOSTIC_STEPS, runDiagnostics } from './diagnostics'
import { type Dataset, makeDataset } from './fixtures'
import { benchmarkProfileName } from './launch'
import {
  type Harness,
  listenBoth,
  nextFrame,
  openScc,
  prepareStores,
  type Profile,
  scaled,
  settleScc,
  sleep,
  verifyState,
} from './harness'
import {
  type BenchmarkMetadata,
  type BenchmarkProgress,
  type BenchmarkReport,
  METHODOLOGY_VERSION,
  summarizeLatencyCase,
  summarizeThroughput,
} from './report'
import { createRng, median, shuffle } from './stats'

const LATENCY_IDLE_MS = 4

const PROFILES: Record<string, Profile> = {
  full: { name: 'full', throughputTrials: 10, latencyTrials: 4, scale: 1 },
  quick: { name: 'quick', throughputTrials: 3, latencyTrials: 1, scale: 0.25 },
}

function selectedProfile(): Profile {
  return PROFILES[benchmarkProfileName()] ?? PROFILES.full!
}

function other(lib: Lib): Lib {
  return lib === 'scc' ? 'mmkv' : 'scc'
}

async function measureThroughput(
  harness: Harness,
  definition: ThroughputCase,
  data: Dataset
): Promise<Record<Lib, number[]>> {
  const chunks = scaled(definition.chunks, harness.profile, 3)
  const first: Lib = harness.rng() < 0.5 ? 'scc' : 'mmkv'
  const setup = definition.listeners
  const probe =
    setup === undefined ? undefined : listenBoth(harness.stores, setup.keys?.(data))
  const samples: Record<Lib, number[]> = { scc: [], mmkv: [] }
  const checksums: Record<Lib, number> = { scc: 0, mmkv: 0 }
  try {
    for (const lib of [first, other(first)]) {
      const body = definition.body(lib, harness.stores, data)
      body(definition.chunkSize, definition.chunkSize * chunks)
      const operations = definition.chunkSize * definition.opsPerCall
      for (let chunk = 0; chunk < chunks; chunk++) {
        const startedAt = performance.now()
        checksums[lib] += body(definition.chunkSize, chunk * definition.chunkSize)
        samples[lib].push(((performance.now() - startedAt) * 1e6) / operations)
      }
      if (lib === 'scc') settleScc(harness.stores.scc)
      if (probe !== undefined && setup !== undefined) {
        await probe.drain(lib, setup.expected(data, definition.chunkSize * (chunks + 1)))
      }
      await nextFrame()
    }
  } finally {
    probe?.remove()
  }
  if (checksums.scc !== checksums.mmkv) {
    throw new Error(
      `${definition.id}: results differ (scc ${checksums.scc}, mmkv ${checksums.mmkv})`
    )
  }
  verifyState(definition.id, harness.stores, definition.verifies?.(data) ?? [])
  return samples
}

async function measureLatency(
  harness: Harness,
  definition: LatencyCase,
  data: Dataset
): Promise<{ samples: Record<Lib, number[]>; gaps: number[] }> {
  const count = scaled(definition.samples, harness.profile, 20)
  const ops = {
    scc: definition.op('scc', harness.stores, data),
    mmkv: definition.op('mmkv', harness.stores, data),
  }
  const first: Lib = harness.rng() < 0.5 ? 'scc' : 'mmkv'
  const samples: Record<Lib, number[]> = { scc: [], mmkv: [] }
  const checksums: Record<Lib, number> = { scc: 0, mmkv: 0 }
  const gaps: number[] = []
  let previousEnd = performance.now()
  for (let index = 0; index < count * 2; index++) {
    await sleep(LATENCY_IDLE_MS)
    const lib = (index & 1) === 0 ? first : other(first)
    const op = ops[lib]
    const startedAt = performance.now()
    checksums[lib] += op(index >> 1)
    const endedAt = performance.now()
    samples[lib].push((endedAt - startedAt) * 1e6)
    gaps.push(startedAt - previousEnd)
    previousEnd = endedAt
  }
  settleScc(harness.stores.scc)
  if (checksums.scc !== checksums.mmkv) {
    throw new Error(`${definition.id}: results differ between libraries`)
  }
  return { samples, gaps }
}

function timerResolutionNs(): number {
  let smallest = Number.POSITIVE_INFINITY
  for (let index = 0; index < 5000; index++) {
    const first = performance.now()
    const delta = performance.now() - first
    if (delta > 0 && delta < smallest) smallest = delta
  }
  return smallest * 1e6
}

function primitiveConstants(): Record<string, string | number | boolean> {
  const out: Record<string, string | number | boolean> = {}
  for (const [key, value] of Object.entries(Platform.constants)) {
    if (['string', 'number', 'boolean'].includes(typeof value)) {
      out[key] = value as string | number | boolean
    }
  }
  return out
}

function hermesVersion(): string | undefined {
  const hermes = (
    globalThis as {
      HermesInternal?: { getRuntimeProperties?: () => Record<string, string> }
    }
  ).HermesInternal
  return hermes?.getRuntimeProperties?.()['OSS Release Version']
}

function reactNativeVersion(): string {
  const { major, minor, patch, prerelease } = Platform.constants.reactNativeVersion
  const base = `${major}.${minor}.${patch}`
  return prerelease === undefined || prerelease === null ? base : `${base}-${prerelease}`
}

function metadata(
  profile: Profile,
  seed: number,
  durationMs: number,
  gaps: number[]
): BenchmarkMetadata {
  return {
    methodologyVersion: METHODOLOGY_VERSION,
    runId: `${Date.now().toString(36)}-${seed.toString(36)}`,
    seed,
    profile: profile.name,
    createdAt: new Date().toISOString(),
    durationMs,
    platform: Platform.OS,
    platformVersion: String(Platform.Version),
    platformConstants: primitiveConstants(),
    buildMode: __DEV__ ? 'development' : 'production',
    hermes: hermesVersion(),
    libraries: {
      scc: sccPackage.version,
      mmkv: mmkvPackage.version,
      nitro: nitroPackage.version,
      reactNative: reactNativeVersion(),
    },
    throughputTrials: profile.throughputTrials,
    latencyTrials: profile.latencyTrials,
    latencyIdleMs: median(gaps),
    timerResolutionNs: timerResolutionNs(),
    sccConfig: "createKV({ persistence: 'wal', durability: 'relaxed' }) · defaults",
    mmkvConfig: "createMMKV({ compareBeforeSet: false }) · library defaults",
  }
}

export async function collectBenchmark(
  onProgress?: (progress: BenchmarkProgress) => void
): Promise<BenchmarkReport> {
  const profile = selectedProfile()
  const seed = (Date.now() ^ Math.floor(Math.random() * 0x7fffffff)) >>> 0
  const rng = createRng(seed)
  const startedAt = performance.now()
  const datasets = { small: makeDataset('small', rng), large: makeDataset('large', rng) }
  const harness: Harness = {
    stores: { scc: openScc(true), mmkv: mmkvBench },
    profile,
    rng,
  }
  const total =
    profile.throughputTrials * throughputCases.length +
    profile.latencyTrials * latencyCases.length +
    DIAGNOSTIC_STEPS
  let completed = 0
  const step = (label: string) => onProgress?.({ completed, total, label })

  const throughput = new Map<string, Record<Lib, number[][]>>()
  const latency = new Map<string, Record<Lib, number[][]>>()
  const gaps: number[] = []
  let diagnostics: BenchmarkReport['diagnostics'] = []

  try {
    for (const scenario of ['small', 'large'] as const) {
      const cases = throughputCases.filter((entry) => entry.scenario === scenario)
      for (let trial = 0; trial < profile.throughputTrials; trial++) {
        step(`${scenario} store · trial ${trial + 1} · seeding`)
        await prepareStores(harness, datasets[scenario])
        for (const definition of shuffle(cases, rng)) {
          step(`${definition.label} · trial ${trial + 1}/${profile.throughputTrials}`)
          const samples = await measureThroughput(harness, definition, datasets[scenario])
          const entry = throughput.get(definition.id) ?? { scc: [], mmkv: [] }
          entry.scc.push(samples.scc)
          entry.mmkv.push(samples.mmkv)
          throughput.set(definition.id, entry)
          completed++
        }
      }
    }

    for (let trial = 0; trial < profile.latencyTrials; trial++) {
      await prepareStores(harness, datasets.small)
      for (const definition of shuffle(latencyCases, rng)) {
        step(`${definition.label} · trial ${trial + 1}/${profile.latencyTrials}`)
        const result = await measureLatency(harness, definition, datasets.small)
        const entry = latency.get(definition.id) ?? { scc: [], mmkv: [] }
        entry.scc.push(result.samples.scc)
        entry.mmkv.push(result.samples.mmkv)
        latency.set(definition.id, entry)
        gaps.push(...result.gaps)
        completed++
      }
    }

    diagnostics = await runDiagnostics(harness, datasets, (label) => {
      step(label)
      completed++
    })
  } finally {
    harness.stores.scc.close()
  }

  step('computing confidence intervals')
  await nextFrame()
  return {
    metadata: metadata(profile, seed, performance.now() - startedAt, gaps),
    throughput: throughputCases.map((definition) =>
      summarizeThroughput(definition, throughput.get(definition.id)!, rng)
    ),
    latency: latencyCases.map((definition) =>
      summarizeLatencyCase(definition, latency.get(definition.id)!, median(gaps), rng)
    ),
    diagnostics,
  }
}
