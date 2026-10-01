#!/usr/bin/env node
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const [beforeDir, afterDir] = process.argv.slice(2)
if (beforeDir === undefined || afterDir === undefined) {
  console.error('usage: node scripts/bench-compare.mjs <before-dir> <after-dir>')
  process.exit(2)
}

function loadReports(dir) {
  return readdirSync(dir)
    .filter((name) => /^launch-\d+\.json$/.test(name))
    .map((name) => JSON.parse(readFileSync(join(dir, name), 'utf8')))
}

const median = (values) => {
  const sorted = [...values].sort((left, right) => left - right)
  const middle = Math.floor(sorted.length / 2)
  return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle]
}

function formatNs(value) {
  if (value === undefined) return '—'
  if (value >= 1e6) return `${(value / 1e6).toFixed(2)} ms`
  if (value >= 1e3) return `${(value / 1e3).toFixed(2)} µs`
  return `${value.toFixed(0)} ns`
}

const delta = (before, after) => {
  if (before === undefined || after === undefined) return '—'
  const change = ((after - before) / before) * 100
  return `${change >= 0 ? '+' : ''}${change.toFixed(0)}%`
}

function pick(reports, section, id, read) {
  const entries = reports.map((report) => report[section].find((entry) => entry.id === id))
  return entries.includes(undefined) ? undefined : median(entries.map(read))
}

const ratio = (mmkv, scc) =>
  mmkv === undefined || scc === undefined ? '—' : `${(mmkv / scc).toFixed(2)}×`

const before = loadReports(beforeDir)
const after = loadReports(afterDir)
const describe = (reports) => {
  const meta = reports[0].metadata
  return `${meta.platform} ${meta.platformVersion} · ${meta.profile} · ${reports.length} launch(es) · SCC ${meta.libraries.scc}`
}

const lines = [
  '# SCC before → after (MMKV = control)',
  '',
  `before: ${describe(before)} · ${beforeDir}`,
  `after: ${describe(after)} · ${afterDir}`,
  '',
  '## Throughput (median of per-launch medians)',
  '',
  '| Case | SCC before | SCC after | SCC Δ | MMKV Δ (noise) | speedup before → after |',
  '| --- | ---: | ---: | ---: | ---: | ---: |',
]
for (const result of after[0].throughput) {
  const read = (lib) => (entry) => entry[lib].median
  const sccBefore = pick(before, 'throughput', result.id, read('scc'))
  const sccAfter = pick(after, 'throughput', result.id, read('scc'))
  const mmkvBefore = pick(before, 'throughput', result.id, read('mmkv'))
  const mmkvAfter = pick(after, 'throughput', result.id, read('mmkv'))
  lines.push(
    `| ${result.label} | ${formatNs(sccBefore)} | ${formatNs(sccAfter)} | ${delta(sccBefore, sccAfter)} | ` +
      `${delta(mmkvBefore, mmkvAfter)} | ${ratio(mmkvBefore, sccBefore)} → ${ratio(mmkvAfter, sccAfter)} |`
  )
}
lines.push(
  '',
  '## Latency after idle',
  '',
  '| Case | SCC p50 before → after | SCC p99 before → after | MMKV p50 Δ (noise) |',
  '| --- | ---: | ---: | ---: |'
)
for (const result of after[0].latency) {
  const value = (reports, lib, stat) =>
    pick(reports, 'latency', result.id, (entry) => entry[lib][stat])
  lines.push(
    `| ${result.label} | ${formatNs(value(before, 'scc', 'p50'))} → ${formatNs(value(after, 'scc', 'p50'))} | ` +
      `${formatNs(value(before, 'scc', 'p99'))} → ${formatNs(value(after, 'scc', 'p99'))} | ` +
      `${delta(value(before, 'mmkv', 'p50'), value(after, 'mmkv', 'p50'))} |`
  )
}
lines.push('', '## SCC-only diagnostics', '', '| Diagnostic | before | after | Δ |', '| --- | ---: | ---: | ---: |')
for (const diagnostic of after[0].diagnostics) {
  const beforeValue = pick(before, 'diagnostics', diagnostic.id, (entry) => entry.median)
  const afterValue = pick(after, 'diagnostics', diagnostic.id, (entry) => entry.median)
  const format = (value) =>
    value === undefined ? '—' : diagnostic.unit === 'ms' ? `${value.toFixed(2)} ms` : formatNs(value)
  lines.push(
    `| ${diagnostic.label} | ${format(beforeValue)} | ${format(afterValue)} | ${delta(beforeValue, afterValue)} |`
  )
}
console.log(lines.join('\n'))
