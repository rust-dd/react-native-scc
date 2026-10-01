import { Text, View } from 'react-native'
import type { Interval } from './bench/stats'
import type {
  BenchmarkProgress,
  BenchmarkReport,
  Diagnostic,
  LatencyResult,
  ThroughputResult,
} from './benchmark'
import type { SelfTestResult } from './self-test'
import { type Palette, styles } from './theme'
import { Card, PillButton } from './ui'

function formatNanoseconds(nanoseconds: number): string {
  if (nanoseconds >= 1_000_000) return `${(nanoseconds / 1_000_000).toFixed(1)} ms`
  if (nanoseconds >= 1_000) {
    return `${(nanoseconds / 1_000).toFixed(nanoseconds >= 10_000 ? 0 : 1)} µs`
  }
  return `${nanoseconds.toFixed(0)} ns`
}

function formatDiagnostic(diagnostic: Diagnostic): string {
  if (diagnostic.unit === 'ns/op') return formatNanoseconds(diagnostic.median)
  return `${diagnostic.median.toFixed(diagnostic.median < 10 ? 2 : 1)} ms`
}

type Tone = 'good' | 'bad' | 'neutral'

function verdict(speedup: Interval): { label: string; tone: Tone } {
  if (speedup.low > 1) {
    return { label: `${speedup.estimate.toFixed(2)}× SCC faster`, tone: 'good' }
  }
  if (speedup.high < 1) {
    return { label: `${(1 / speedup.estimate).toFixed(2)}× SCC slower`, tone: 'bad' }
  }
  return { label: 'no clear difference', tone: 'neutral' }
}

function intervalText(speedup: Interval): string {
  return `95% CI ${speedup.low.toFixed(2)}–${speedup.high.toFixed(2)}×`
}

function ComparisonRow({
  label,
  detail,
  notes,
  scc,
  mmkv,
  speedup,
  t,
}: {
  label: string
  detail: string
  notes: string[]
  scc: number
  mmkv: number
  speedup: Interval
  t: Palette
}) {
  const max = Math.max(scc, mmkv, 1)
  const { label: verdictLabel, tone } = verdict(speedup)
  const chipColor = tone === 'good' ? t.good : tone === 'bad' ? t.bad : t.sub
  const chipBackground =
    tone === 'good' ? t.goodSoft : tone === 'bad' ? t.badSoft : t.track

  return (
    <View
      accessibilityLabel={`${label}. SCC ${formatNanoseconds(scc)}, MMKV ${formatNanoseconds(mmkv)}. ${verdictLabel}, ${intervalText(speedup)}`}
      accessible
      style={styles.benchRow}
    >
      <View style={styles.benchHeader}>
        <View style={styles.benchTitleWrap}>
          <Text style={[styles.benchName, { color: t.ink }]}>{label}</Text>
          <Text style={[styles.benchDetail, { color: t.faint }]}>{detail}</Text>
          {notes.map((note) => (
            <Text key={note} style={[styles.benchDetail, { color: t.faint }]}>
              {note}
            </Text>
          ))}
        </View>
        <View style={[styles.chip, { backgroundColor: chipBackground }]}>
          <Text style={[styles.chipLabel, { color: chipColor }]}>
            {verdictLabel}
          </Text>
        </View>
      </View>
      {(
        [
          ['scc', scc, t.accent],
          ['mmkv', mmkv, t.mmkv],
        ] as const
      ).map(([series, value, color]) => {
        const width = `${Math.max(1, (value / max) * 100)}%` as `${number}%`
        return (
          <View key={series} style={styles.barRow}>
            <Text style={[styles.barLabel, { color: t.faint }]}>{series}</Text>
            <View style={[styles.barTrack, { backgroundColor: t.track }]}>
              <View
                style={[styles.barFill, { backgroundColor: color, width }]}
              />
            </View>
            <Text style={[styles.barValue, { color: t.sub }]}>
              {formatNanoseconds(value)}
            </Text>
          </View>
        )
      })}
    </View>
  )
}

function ThroughputRow({ result, t }: { result: ThroughputResult; t: Palette }) {
  const notes = [
    `${intervalText(result.speedup)} · p95 chunk SCC ${formatNanoseconds(result.scc.p95Chunk)} · MMKV ${formatNanoseconds(result.mmkv.p95Chunk)}`,
  ]
  if (result.opsPerCall > 1) {
    notes.push(
      `per call · SCC ${formatNanoseconds(result.scc.median * result.opsPerCall)} · MMKV ${formatNanoseconds(result.mmkv.median * result.opsPerCall)}`
    )
  }
  return (
    <ComparisonRow
      detail={result.detail}
      label={result.label}
      mmkv={result.mmkv.median}
      notes={notes}
      scc={result.scc.median}
      speedup={result.speedup}
      t={t}
    />
  )
}

function LatencyRow({ result, t }: { result: LatencyResult; t: Palette }) {
  return (
    <ComparisonRow
      detail={`${result.detail} · ~${result.gapMs.toFixed(1)} ms idle`}
      label={result.label}
      mmkv={result.mmkv.p50}
      notes={[
        `p50 ${intervalText(result.speedupP50)}`,
        `p99 · SCC ${formatNanoseconds(result.scc.p99)} · MMKV ${formatNanoseconds(result.mmkv.p99)} · ${verdict(result.speedupP99).label}`,
      ]}
      scc={result.scc.p50}
      speedup={result.speedupP50}
      t={t}
    />
  )
}

function DiagnosticRow({ diagnostic, t }: { diagnostic: Diagnostic; t: Palette }) {
  return (
    <View
      accessibilityLabel={`${diagnostic.label}: ${formatDiagnostic(diagnostic)}. ${diagnostic.detail}`}
      accessible
      style={styles.testRow}
    >
      <Text style={[styles.testName, { color: t.sub }]}>{diagnostic.label}</Text>
      <Text style={[styles.testDetail, { color: t.ink }]}>
        {formatDiagnostic(diagnostic)}
      </Text>
    </View>
  )
}

function GroupTitle({ title, t }: { title: string; t: Palette }) {
  return (
    <Text accessibilityRole="header" style={[styles.benchGroup, { color: t.faint }]}>
      {title}
    </Text>
  )
}

export function SelfTestSection({
  results,
  running,
  error,
  onRun,
  t,
}: {
  results: SelfTestResult[]
  running: boolean
  error?: string
  onRun: () => void
  t: Palette
}) {
  const passed = results.filter((result) => result.ok).length

  return (
    <Card
      caption={
        running
          ? `${passed} checks passed so far`
          : `${passed}/${results.length} passed`
      }
      t={t}
      title="SELF-TEST"
    >
      {error !== undefined && (
        <Text
          accessibilityRole="alert"
          style={[styles.sectionMessage, { color: t.bad }]}
        >
          {error}
        </Text>
      )}
      {running && results.length === 0 && (
        <Text style={[styles.sectionMessage, { color: t.faint }]}>
          Exercising sync, async, batch, TTL, encryption and adapter paths…
        </Text>
      )}
      {results.map((result) => (
        <View
          accessibilityLabel={`${result.ok ? 'Passed' : 'Failed'}: ${result.name}${result.detail === '' ? '' : `. ${result.detail}`}`}
          accessible
          key={result.name}
          style={styles.testRow}
        >
          <Text
            style={[styles.testTick, { color: result.ok ? t.good : t.bad }]}
          >
            {result.ok ? '✓' : '✕'}
          </Text>
          <Text style={[styles.testName, { color: result.ok ? t.sub : t.bad }]}>
            {result.name}
          </Text>
          {result.detail !== '' && (
            <Text style={[styles.testDetail, { color: t.faint }]}>
              {result.detail}
            </Text>
          )}
        </View>
      ))}
      <View style={styles.sectionActions}>
        <PillButton
          accessibilityHint="Runs all storage checks again"
          disabled={running}
          label={running ? 'Self-test running…' : 'Run self-test again'}
          onPress={onRun}
          t={t}
        />
      </View>
    </Card>
  )
}

function BenchmarkMetadataText({
  report,
  t,
}: {
  report: BenchmarkReport
  t: Palette
}) {
  const metadata = report.metadata
  const libraries = metadata.libraries
  return (
    <Text style={[styles.metadata, { color: t.faint }]}>
      {metadata.platform} {metadata.platformVersion} · {metadata.buildMode} · RN{' '}
      {libraries.reactNative} · Hermes {metadata.hermes ?? 'n/a'} · SCC{' '}
      {libraries.scc} · MMKV {libraries.mmkv} · Nitro {libraries.nitro} ·{' '}
      {metadata.profile} profile: {metadata.throughputTrials} throughput /{' '}
      {metadata.latencyTrials} latency trials, randomized order · timer{' '}
      {metadata.timerResolutionNs.toFixed(0)} ns ·{' '}
      {new Date(metadata.createdAt).toLocaleString()}
    </Text>
  )
}

export function BenchmarkSection({
  report,
  running,
  error,
  progress,
  onRun,
  t,
}: {
  report?: BenchmarkReport
  running: boolean
  error?: string
  progress?: BenchmarkProgress
  onRun: () => void
  t: Palette
}) {
  const progressRatio =
    progress === undefined || progress.total === 0
      ? 0
      : progress.completed / progress.total
  const progressWidth = `${Math.max(2, progressRatio * 100)}%` as `${number}%`
  const small = report?.throughput.filter((result) => result.scenario === 'small') ?? []
  const large = report?.throughput.filter((result) => result.scenario === 'large') ?? []

  return (
    <Card
      caption="randomized trials · 95% CI · lower is better"
      t={t}
      title="BENCHMARK · VS MMKV"
    >
      {__DEV__ && (
        <View style={[styles.warning, { backgroundColor: t.badSoft }]}>
          <Text style={[styles.warningText, { color: t.bad }]}>
            Development builds distort timings. Use an iOS or Android Release
            build for meaningful comparisons.
          </Text>
        </View>
      )}

      {error !== undefined && (
        <Text
          accessibilityRole="alert"
          style={[styles.sectionMessage, { color: t.bad }]}
        >
          Benchmark failed: {error}
        </Text>
      )}

      {running && (
        <View
          accessibilityLabel={`Benchmark ${progress?.completed ?? 0} of ${progress?.total ?? 0}: ${progress?.label ?? 'preparing'}`}
          accessibilityRole="progressbar"
          accessibilityValue={{
            min: 0,
            max: progress?.total ?? 1,
            now: progress?.completed ?? 0,
          }}
          style={styles.progress}
        >
          <Text style={[styles.caption, { color: t.sub }]}>
            {progress === undefined
              ? 'Preparing identical stores…'
              : `${progress.label} · ${progress.completed}/${progress.total}`}
          </Text>
          <View style={[styles.progressTrack, { backgroundColor: t.track }]}>
            <View
              style={[
                styles.progressFill,
                { backgroundColor: t.accent, width: progressWidth },
              ]}
            />
          </View>
        </View>
      )}

      {!running && report === undefined && error === undefined && (
        <Text style={[styles.sectionMessage, { color: t.faint }]}>
          Run manually when the device is idle and cool. Takes a few minutes;
          results are saved with build and platform metadata.
        </Text>
      )}

      {report !== undefined && (
        <>
          <BenchmarkMetadataText report={report} t={t} />
          <GroupTitle t={t} title="THROUGHPUT · SMALL STORE" />
          {small.map((result) => (
            <ThroughputRow key={result.id} result={result} t={t} />
          ))}
          <GroupTitle t={t} title="THROUGHPUT · 20K-KEY STORE" />
          {large.map((result) => (
            <ThroughputRow key={result.id} result={result} t={t} />
          ))}
          <GroupTitle t={t} title="LATENCY AFTER IDLE · P50" />
          {report.latency.map((result) => (
            <LatencyRow key={result.id} result={result} t={t} />
          ))}
          <GroupTitle t={t} title="SCC-ONLY DIAGNOSTICS · MEDIAN" />
          {report.diagnostics.map((diagnostic) => (
            <DiagnosticRow diagnostic={diagnostic} key={diagnostic.id} t={t} />
          ))}
        </>
      )}

      <View style={styles.sectionActions}>
        <PillButton
          accessibilityHint="Measures SCC and MMKV in randomized interleaved trials"
          disabled={running}
          label={running ? 'Benchmark running…' : 'Run benchmark'}
          onPress={onRun}
          t={t}
        />
      </View>
    </Card>
  )
}
