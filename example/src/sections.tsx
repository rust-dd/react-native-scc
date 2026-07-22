import { Text, View } from 'react-native'
import type {
  BenchmarkCase,
  BenchmarkProgress,
  BenchmarkReport,
} from './benchmark'
import type { SelfTestResult } from './self-test'
import { type Palette, styles } from './theme'
import { Card, PillButton } from './ui'

function formatNanoseconds(nanoseconds: number): string {
  return nanoseconds >= 1_000
    ? `${(nanoseconds / 1_000).toFixed(1)} µs`
    : `${nanoseconds.toFixed(0)} ns`
}

function BenchmarkRow({ result, t }: { result: BenchmarkCase; t: Palette }) {
  const max = Math.max(result.scc, result.mmkv, 1)
  const isTie = Math.abs(result.scc - result.mmkv) / max < 0.02
  const sccIsFaster = result.scc < result.mmkv
  const ratio = sccIsFaster
    ? result.mmkv / result.scc
    : result.scc / result.mmkv
  const chipColor = isTie ? t.sub : sccIsFaster ? t.good : t.bad
  const chipBackground = isTie ? t.track : sccIsFaster ? t.goodSoft : t.badSoft
  const verdict = isTie
    ? '≈ tie'
    : `${ratio.toFixed(1)}× SCC ${sccIsFaster ? 'faster' : 'slower'}`

  return (
    <View
      accessibilityLabel={`${result.label}. SCC ${formatNanoseconds(result.scc)}, MMKV ${formatNanoseconds(result.mmkv)}. ${verdict}`}
      accessible
      style={styles.benchRow}
    >
      <View style={styles.benchHeader}>
        <View style={styles.benchTitleWrap}>
          <Text style={[styles.benchName, { color: t.ink }]}>
            {result.label}
          </Text>
          <Text style={[styles.benchDetail, { color: t.faint }]}>
            {result.detail}
          </Text>
          {result.operationsPerCall > 1 && (
            <Text style={[styles.benchDetail, { color: t.faint }]}>
              total per call · SCC{' '}
              {formatNanoseconds(result.scc * result.operationsPerCall)} · MMKV{' '}
              {formatNanoseconds(result.mmkv * result.operationsPerCall)}
            </Text>
          )}
        </View>
        <View style={[styles.chip, { backgroundColor: chipBackground }]}>
          <Text style={[styles.chipLabel, { color: chipColor }]}>
            {verdict}
          </Text>
        </View>
      </View>
      {(
        [
          ['scc', result.scc, t.accent],
          ['mmkv', result.mmkv, t.mmkv],
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
  return (
    <Text style={[styles.metadata, { color: t.faint }]}>
      {metadata.platform} {metadata.platformVersion} · {metadata.buildMode} ·{' '}
      {metadata.trials}-trial median · {metadata.seededKeys} seeded keys ·{' '}
      fresh SCC store/trial · sync API latency · relaxed WAL · drain outside
      timing ·{' '}
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

  return (
    <Card
      caption="4-trial median · balanced order · lower is better"
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
          Run manually when the device is idle. Results are saved with build and
          platform metadata.
        </Text>
      )}

      {report !== undefined && (
        <>
          <BenchmarkMetadataText report={report} t={t} />
          {report.results.map((result) => (
            <BenchmarkRow key={result.id} result={result} t={t} />
          ))}
        </>
      )}

      <View style={styles.sectionActions}>
        <PillButton
          accessibilityHint="Measures SCC and MMKV using four balanced alternating trials"
          disabled={running}
          label={running ? 'Benchmark running…' : 'Run benchmark'}
          onPress={onRun}
          t={t}
        />
      </View>
    </Card>
  )
}
