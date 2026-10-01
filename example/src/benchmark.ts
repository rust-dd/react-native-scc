import {
  type BenchmarkProgress,
  type BenchmarkReport,
  isBenchmarkReport,
} from './bench/report'
import { collectBenchmark } from './bench/runner'
import { kv } from './storage'

export type {
  BenchmarkProgress,
  BenchmarkReport,
  Diagnostic,
  LatencyResult,
  ThroughputResult,
} from './bench/report'

// scripts/bench-ios.mjs reads the report from the store files under this key.
export const REPORT_KEY = 'last_bench'

export function getLastBenchmark(): BenchmarkReport | undefined {
  const stored = kv.getJSON<unknown>(REPORT_KEY)
  return isBenchmarkReport(stored) ? stored : undefined
}

function logSummary(report: BenchmarkReport): void {
  for (const result of report.throughput) {
    const { estimate, low, high } = result.speedup
    console.log(
      `SCC_BENCH case=${result.id} scc=${result.scc.median.toFixed(0)}ns mmkv=${result.mmkv.median.toFixed(0)}ns speedup=${estimate.toFixed(2)} ci=${low.toFixed(2)}-${high.toFixed(2)}`
    )
  }
  for (const result of report.latency) {
    console.log(
      `SCC_BENCH latency=${result.id} p50 scc=${result.scc.p50.toFixed(0)}ns mmkv=${result.mmkv.p50.toFixed(0)}ns p99 scc=${result.scc.p99.toFixed(0)}ns mmkv=${result.mmkv.p99.toFixed(0)}ns`
    )
  }
}

export async function runBenchmark(
  onProgress?: (progress: BenchmarkProgress) => void
): Promise<BenchmarkReport> {
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
  console.log('SCC_BENCH_START')
  const report = await collectBenchmark(onProgress)
  kv.setJSON(REPORT_KEY, report)
  kv.flush()
  logSummary(report)
  console.log(`SCC_BENCH_DONE runId=${report.metadata.runId}`)
  return report
}
