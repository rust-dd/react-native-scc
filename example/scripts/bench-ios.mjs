#!/usr/bin/env node
import { execFileSync, spawnSync } from 'node:child_process'
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const BUNDLE_ID = 'com.rustdd.scckv.example'
const STORE_DIR = 'Library/Application Support/react-native-scc'
const STORE_ID = 'example'
const REPORT_KEY = 'last_bench'

const exampleDir = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repoDir = resolve(exampleDir, '..')

const { values: options } = parseArgs({
  options: {
    device: { type: 'string' },
    launches: { type: 'string', default: '3' },
    profile: { type: 'string', default: 'full' },
    cooldown: { type: 'string', default: '30' },
    timeout: { type: 'string', default: '1800' },
    out: { type: 'string', default: join(exampleDir, 'bench-results') },
    'skip-build': { type: 'boolean', default: false },
    team: { type: 'string' },
    help: { type: 'boolean', default: false },
  },
})

if (options.help) {
  console.log(`usage: node scripts/bench-ios.mjs [--device <udid|name>] [--launches 3]
  [--profile full|quick] [--cooldown <s>] [--timeout <s>] [--out <dir>] [--skip-build]
  [--team <apple-team-id>]

Defaults to the booted simulator. A physical device needs --team (Xcode signing team) for the
build; list teams with: defaults read com.apple.dt.Xcode IDEProvisioningTeams`)
  process.exit(0)
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms))

function run(command, args, extra = {}) {
  return execFileSync(command, args, {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    maxBuffer: 64 * 1024 * 1024,
    ...extra,
  })
}

function resolveTarget(query) {
  const listed = JSON.parse(run('xcrun', ['simctl', 'list', 'devices', 'available', '-j']))
  const simulators = Object.values(listed.devices).flat()
  const simulator =
    query === undefined
      ? simulators.find((entry) => entry.state === 'Booted')
      : simulators.find((entry) => entry.udid === query || entry.name === query)
  if (simulator !== undefined) {
    return { kind: 'simulator', udid: simulator.udid, name: simulator.name }
  }
  if (query === undefined) throw new Error('no booted simulator: pass --device <udid|name>')

  const scratch = mkdtempSync(join(tmpdir(), 'scc-bench-'))
  try {
    const listing = join(scratch, 'devices.json')
    run('xcrun', ['devicectl', 'list', 'devices', '--json-output', listing])
    const devices = JSON.parse(readFileSync(listing, 'utf8')).result?.devices ?? []
    const device = devices.find(
      (entry) =>
        entry.identifier === query ||
        entry.hardwareProperties?.udid === query ||
        entry.deviceProperties?.name === query
    )
    if (device === undefined) throw new Error(`no simulator or device matches "${query}"`)
    return {
      kind: 'device',
      udid: device.hardwareProperties?.udid ?? device.identifier,
      id: device.identifier,
      name: device.deviceProperties?.name ?? query,
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}

function buildApp(target) {
  if (target.kind === 'simulator') {
    const result = spawnSync(
      'npx',
      ['expo', 'run:ios', '--configuration', 'Release', '--device', target.udid, '--no-bundler'],
      { cwd: exampleDir, stdio: 'inherit' }
    )
    if (result.status !== 0) throw new Error('expo run:ios failed')
    return
  }
  // expo run:ios prompts for a signing team on devices; xcodebuild takes it non-interactively.
  if (options.team === undefined) throw new Error('a physical device build needs --team <id>')
  const derivedData = join(exampleDir, 'ios', 'build')
  const build = spawnSync(
    'xcodebuild',
    [
      '-workspace', join(exampleDir, 'ios', 'sccexample.xcworkspace'),
      '-scheme', 'sccexample',
      '-configuration', 'Release',
      '-destination', `id=${target.udid}`,
      '-derivedDataPath', derivedData,
      '-allowProvisioningUpdates',
      `DEVELOPMENT_TEAM=${options.team}`,
      'build',
    ],
    { cwd: exampleDir, stdio: 'inherit' }
  )
  if (build.status !== 0) throw new Error('xcodebuild failed')
  const app = join(derivedData, 'Build', 'Products', 'Release-iphoneos', 'sccexample.app')
  run('xcrun', ['devicectl', 'device', 'install', 'app', '--device', target.id, app])
}

function launchArguments() {
  return ['-sccBenchAutorun', '1', '-sccBenchProfile', options.profile]
}

function terminate(target) {
  if (target.kind === 'simulator') {
    spawnSync('xcrun', ['simctl', 'terminate', target.udid, BUNDLE_ID], { stdio: 'ignore' })
  }
}

function launch(target) {
  if (target.kind === 'simulator') {
    run('xcrun', ['simctl', 'launch', target.udid, BUNDLE_ID, ...launchArguments()])
  } else {
    run('xcrun', [
      'devicectl', 'device', 'process', 'launch',
      '--device', target.id, '--terminate-existing', BUNDLE_ID, ...launchArguments(),
    ])
  }
}

function pullStore(target, into) {
  for (const extension of ['snap', 'wal']) {
    const file = `${STORE_ID}.${extension}`
    if (target.kind === 'simulator') {
      const container = run('xcrun', [
        'simctl', 'get_app_container', target.udid, BUNDLE_ID, 'data',
      ]).trim()
      const source = join(container, STORE_DIR, file)
      if (existsSync(source)) copyFileSync(source, join(into, file))
    } else {
      spawnSync(
        'xcrun',
        [
          'devicectl', 'device', 'copy', 'from',
          '--device', target.id,
          '--domain-type', 'appDataContainer',
          '--domain-identifier', BUNDLE_ID,
          '--source', `${STORE_DIR}/${file}`,
          '--destination', join(into, file),
        ],
        { stdio: 'ignore' }
      )
    }
  }
}

function readReport(target, dumpBinary) {
  const scratch = mkdtempSync(join(tmpdir(), 'scc-bench-store-'))
  try {
    pullStore(target, scratch)
    const result = spawnSync(dumpBinary, [scratch, STORE_ID, REPORT_KEY], {
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
    })
    return result.status === 0 ? JSON.parse(result.stdout) : undefined
  } catch {
    return undefined
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}

async function waitForReport(target, dumpBinary, previousRunId, timeoutMs) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    await sleep(10_000)
    const report = readReport(target, dumpBinary)
    const runId = report?.metadata?.runId
    if (runId !== undefined && runId !== previousRunId) return report
    process.stdout.write('.')
  }
  throw new Error('timed out waiting for a new benchmark report (did the self-test fail?)')
}

const median = (values) => {
  const sorted = [...values].sort((left, right) => left - right)
  const middle = Math.floor(sorted.length / 2)
  return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle]
}

function formatNs(value) {
  if (value >= 1e6) return `${(value / 1e6).toFixed(2)} ms`
  if (value >= 1e3) return `${(value / 1e3).toFixed(2)} µs`
  return `${value.toFixed(0)} ns`
}

function spread(values, format) {
  if (values.length === 1) return format(values[0])
  return `${format(median(values))} (${format(Math.min(...values))}–${format(Math.max(...values))})`
}

function verdicts(intervals) {
  return intervals
    .map(({ low, high }) => (low > 1 ? 'SCC' : high < 1 ? 'MMKV' : 'tie'))
    .join(' / ')
}

function summarize(reports, target) {
  const first = reports[0]
  const meta = first.metadata
  const lines = [
    '# react-native-scc vs react-native-mmkv',
    '',
    `${target.name} (${target.kind}) · ${meta.platform} ${meta.platformVersion} · ${meta.buildMode} build · ` +
      `profile ${meta.profile} · ${reports.length} launch(es)`,
    `SCC ${meta.libraries.scc} · MMKV ${meta.libraries.mmkv} · Nitro ${meta.libraries.nitro} · ` +
      `RN ${meta.libraries.reactNative} · Hermes ${meta.hermes ?? 'n/a'} · timer ${meta.timerResolutionNs.toFixed(0)} ns`,
    '',
    'Values: median of per-launch medians (min–max across launches). Lower is better. ',
    '"Faster" lists, per launch, which library the 95% bootstrap CI favors ("tie" = CI contains 1×).',
    '',
    '## Throughput (ns per operation)',
    '',
    '| Case | Store | SCC | MMKV | SCC speedup | Faster |',
    '| --- | --- | ---: | ---: | ---: | --- |',
  ]
  for (const [index, result] of first.throughput.entries()) {
    const perLaunch = reports.map((report) => report.throughput[index])
    lines.push(
      `| ${result.label} | ${result.scenario} | ${spread(perLaunch.map((entry) => entry.scc.median), formatNs)} | ` +
        `${spread(perLaunch.map((entry) => entry.mmkv.median), formatNs)} | ` +
        `${spread(perLaunch.map((entry) => entry.speedup.estimate), (value) => `${value.toFixed(2)}×`)} | ` +
        `${verdicts(perLaunch.map((entry) => entry.speedup))} |`
    )
  }
  lines.push(
    '',
    `## Latency after idle (~${meta.latencyIdleMs.toFixed(1)} ms idle event loop before each call)`,
    '',
    '| Case | SCC p50 | MMKV p50 | SCC p99 | MMKV p99 | p50 faster | p99 faster |',
    '| --- | ---: | ---: | ---: | ---: | --- | --- |'
  )
  for (const [index, result] of first.latency.entries()) {
    const perLaunch = reports.map((report) => report.latency[index])
    lines.push(
      `| ${result.label} | ${spread(perLaunch.map((entry) => entry.scc.p50), formatNs)} | ` +
        `${spread(perLaunch.map((entry) => entry.mmkv.p50), formatNs)} | ` +
        `${spread(perLaunch.map((entry) => entry.scc.p99), formatNs)} | ` +
        `${spread(perLaunch.map((entry) => entry.mmkv.p99), formatNs)} | ` +
        `${verdicts(perLaunch.map((entry) => entry.speedupP50))} | ` +
        `${verdicts(perLaunch.map((entry) => entry.speedupP99))} |`
    )
  }
  lines.push('', '## SCC-only diagnostics', '', '| Diagnostic | Median |', '| --- | ---: |')
  for (const [index, diagnostic] of first.diagnostics.entries()) {
    const values = reports.map((report) => report.diagnostics[index].median)
    const format = (value) =>
      diagnostic.unit === 'ms' ? `${value.toFixed(2)} ms` : formatNs(value)
    lines.push(`| ${diagnostic.label} | ${spread(values, format)} |`)
  }
  return `${lines.join('\n')}\n`
}

async function main() {
  const target = resolveTarget(options.device)
  const launches = Number(options.launches)
  console.log(`target: ${target.name} (${target.kind} ${target.udid})`)

  execFileSync('cargo', ['build', '--release', '-p', 'kv-core', '--example', 'dump_value'], {
    cwd: repoDir,
    stdio: 'inherit',
  })
  const dumpBinary = join(repoDir, 'target', 'release', 'examples', 'dump_value')

  if (!options['skip-build']) buildApp(target)

  const outDir = join(
    options.out,
    `${new Date().toISOString().replace(/[:.]/g, '-')}-${target.name.replace(/\W+/g, '-')}`
  )
  mkdirSync(outDir, { recursive: true })

  const reports = []
  let previousRunId = readReport(target, dumpBinary)?.metadata?.runId
  for (let index = 1; index <= launches; index++) {
    if (index > 1) {
      console.log(`cooling down ${options.cooldown}s`)
      await sleep(Number(options.cooldown) * 1000)
    }
    terminate(target)
    launch(target)
    process.stdout.write(`launch ${index}/${launches}: waiting for report`)
    const report = await waitForReport(
      target,
      dumpBinary,
      previousRunId,
      Number(options.timeout) * 1000
    )
    process.stdout.write(` done (${(report.metadata.durationMs / 1000).toFixed(0)} s)\n`)
    previousRunId = report.metadata.runId
    writeFileSync(join(outDir, `launch-${index}.json`), JSON.stringify(report))
    reports.push(report)
  }
  terminate(target)

  const summary = summarize(reports, target)
  writeFileSync(join(outDir, 'summary.md'), summary)
  console.log(`\n${summary}\nraw reports: ${outDir}`)
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error)
  process.exit(1)
})
