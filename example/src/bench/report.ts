import type { ThroughputCase } from './cases'
import {
  type Interval,
  median,
  quantile,
  type Rng,
  speedupInterval,
} from './stats'

export const METHODOLOGY_VERSION = 6

export interface BenchmarkMetadata {
  methodologyVersion: typeof METHODOLOGY_VERSION
  runId: string
  seed: number
  profile: string
  createdAt: string
  durationMs: number
  platform: string
  platformVersion: string
  platformConstants: Record<string, string | number | boolean>
  buildMode: 'development' | 'production'
  hermes?: string
  libraries: Record<'scc' | 'mmkv' | 'nitro' | 'reactNative', string>
  throughputTrials: number
  latencyTrials: number
  latencyIdleMs: number
  timerResolutionNs: number
  sccConfig: string
  mmkvConfig: string
}

export interface LibSummary {
  median: number
  min: number
  max: number
  p95Chunk: number
}

export interface ThroughputResult {
  id: string
  scenario: ThroughputCase['scenario']
  group: ThroughputCase['group']
  label: string
  detail: string
  opsPerCall: number
  chunkSize: number
  chunks: number
  scc: LibSummary
  mmkv: LibSummary
  speedup: Interval
  samples: Record<'scc' | 'mmkv', number[][]>
}

export interface LatencySummary {
  p50: number
  p90: number
  p99: number
}

export interface LatencyResult {
  id: string
  label: string
  detail: string
  gapMs: number
  scc: LatencySummary
  mmkv: LatencySummary
  speedupP50: Interval
  speedupP99: Interval
  samples: Record<'scc' | 'mmkv', number[][]>
}

export interface Diagnostic {
  id: string
  label: string
  detail: string
  unit: 'ns/op' | 'ms'
  values: number[]
  median: number
}

export interface BenchmarkReport {
  metadata: BenchmarkMetadata
  throughput: ThroughputResult[]
  latency: LatencyResult[]
  diagnostics: Diagnostic[]
}

export interface BenchmarkProgress {
  completed: number
  total: number
  label: string
}

function summarizeLib(trials: number[][]): LibSummary {
  const trialMedians = trials.map(median)
  return {
    median: median(trialMedians),
    min: Math.min(...trialMedians),
    max: Math.max(...trialMedians),
    p95Chunk: quantile(trials.flat(), 0.95),
  }
}

export function summarizeThroughput(
  definition: ThroughputCase,
  samples: Record<'scc' | 'mmkv', number[][]>,
  rng: Rng
): ThroughputResult {
  return {
    id: definition.id,
    scenario: definition.scenario,
    group: definition.group,
    label: definition.label,
    detail: definition.detail,
    opsPerCall: definition.opsPerCall,
    chunkSize: definition.chunkSize,
    chunks: definition.chunks,
    scc: summarizeLib(samples.scc),
    mmkv: summarizeLib(samples.mmkv),
    speedup: speedupInterval(
      samples.scc.map(median),
      samples.mmkv.map(median),
      rng
    ),
    samples,
  }
}

function summarizeLatency(values: number[]): LatencySummary {
  return {
    p50: quantile(values, 0.5),
    p90: quantile(values, 0.9),
    p99: quantile(values, 0.99),
  }
}

export function summarizeLatencyCase(
  definition: { id: string; label: string; detail: string },
  samples: Record<'scc' | 'mmkv', number[][]>,
  gapMs: number,
  rng: Rng
): LatencyResult {
  const scc = samples.scc.flat()
  const mmkv = samples.mmkv.flat()
  const p99 = (values: readonly number[]) => quantile(values, 0.99)
  return {
    id: definition.id,
    label: definition.label,
    detail: definition.detail,
    gapMs,
    scc: summarizeLatency(scc),
    mmkv: summarizeLatency(mmkv),
    speedupP50: speedupInterval(scc, mmkv, rng, 500),
    speedupP99: speedupInterval(scc, mmkv, rng, 500, p99),
    samples,
  }
}

export function isBenchmarkReport(value: unknown): value is BenchmarkReport {
  if (value === null || typeof value !== 'object') return false
  const report = value as Partial<BenchmarkReport>
  return (
    report.metadata?.methodologyVersion === METHODOLOGY_VERSION &&
    Array.isArray(report.throughput) &&
    Array.isArray(report.latency) &&
    Array.isArray(report.diagnostics)
  )
}
