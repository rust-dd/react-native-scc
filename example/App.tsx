import { StatusBar } from 'expo-status-bar'
import { useCallback, useEffect, useRef, useState } from 'react'
import { ScrollView, Text, useColorScheme, View } from 'react-native'
import {
  initialWindowMetrics,
  SafeAreaProvider,
  SafeAreaView,
} from 'react-native-safe-area-context'
import {
  getLastBenchmark,
  runBenchmark,
  type BenchmarkProgress,
  type BenchmarkReport,
} from './src/benchmark'
import { Demos } from './src/demos'
import { runSelfTest, type SelfTestResult } from './src/self-test'
import { BenchmarkSection, SelfTestSection } from './src/sections'
import { palettes, styles } from './src/theme'

const autorunBenchmark = process.env.EXPO_PUBLIC_SCC_AUTORUN_BENCHMARK === '1'

export default function App() {
  const colorScheme = useColorScheme()
  const t = palettes[colorScheme === 'dark' ? 'dark' : 'light']

  const [selfTestResults, setSelfTestResults] = useState<SelfTestResult[]>([])
  const [selfTestRunning, setSelfTestRunning] = useState(true)
  const [selfTestError, setSelfTestError] = useState<string>()
  const selfTestRun = useRef(0)

  const [benchmarkReport, setBenchmarkReport] = useState<
    BenchmarkReport | undefined
  >(getLastBenchmark)
  const [benchmarkRunning, setBenchmarkRunning] = useState(false)
  const [benchmarkError, setBenchmarkError] = useState<string>()
  const [benchmarkProgress, setBenchmarkProgress] =
    useState<BenchmarkProgress>()
  const benchmarkRun = useRef(0)

  const executeBenchmark = useCallback(async () => {
    const run = ++benchmarkRun.current
    setBenchmarkRunning(true)
    setBenchmarkError(undefined)
    setBenchmarkProgress(undefined)

    try {
      const report = await runBenchmark((progress) => {
        if (benchmarkRun.current === run) setBenchmarkProgress(progress)
      })
      if (benchmarkRun.current === run) setBenchmarkReport(report)
    } catch (error) {
      const message = String(error)
      console.error(`SCC_BENCH_FAIL: ${message}`)
      if (benchmarkRun.current === run) setBenchmarkError(message)
    } finally {
      if (benchmarkRun.current === run) {
        setBenchmarkRunning(false)
        setBenchmarkProgress(undefined)
      }
    }
  }, [])

  const executeSelfTest = useCallback(
    async (runBenchmarkAfterSuccess = false) => {
      const run = ++selfTestRun.current
      setSelfTestResults([])
      setSelfTestRunning(true)
      setSelfTestError(undefined)

      try {
        const results = await runSelfTest((partialResults) => {
          if (selfTestRun.current === run) setSelfTestResults(partialResults)
        })
        if (selfTestRun.current !== run) return

        setSelfTestResults(results)
        const failed = results.filter((result) => !result.ok)
        if (failed.length === 0) {
          console.log('SCC_SELFTEST_OK')
          if (runBenchmarkAfterSuccess) {
            setSelfTestRunning(false)
            console.log('SCC_BENCH_AUTORUN=1')
            await executeBenchmark()
          }
        } else {
          console.error(
            `SCC_SELFTEST_FAIL: ${failed.map((result) => result.name).join(', ')}`
          )
          if (runBenchmarkAfterSuccess) {
            console.error('SCC_BENCH_AUTORUN_SKIPPED=self-test-failed')
          }
        }
      } catch (error) {
        const message = String(error)
        console.error(`SCC_SELFTEST_FAIL: ${message}`)
        if (selfTestRun.current === run) setSelfTestError(message)
      } finally {
        if (selfTestRun.current === run) setSelfTestRunning(false)
      }
    },
    [executeBenchmark]
  )

  useEffect(() => {
    void executeSelfTest(autorunBenchmark)
    return () => {
      selfTestRun.current += 1
      benchmarkRun.current += 1
    }
  }, [executeSelfTest])

  const passed = selfTestResults.filter((result) => result.ok).length
  const failed = selfTestResults.length - passed
  const statusColor = selfTestRunning
    ? t.faint
    : selfTestError !== undefined || failed > 0
      ? t.bad
      : t.good
  const statusLabel = selfTestRunning
    ? 'checking…'
    : selfTestError !== undefined
      ? 'self-test error'
      : failed > 0
        ? `${failed} failing`
        : 'all passing'

  return (
    <SafeAreaProvider initialMetrics={initialWindowMetrics}>
      <StatusBar style="auto" />
      <SafeAreaView
        edges={['top', 'right', 'bottom', 'left']}
        style={[styles.screen, { backgroundColor: t.bg }]}
      >
        <ScrollView
          contentContainerStyle={styles.scroll}
          contentInsetAdjustmentBehavior="automatic"
        >
          <View style={styles.content}>
            <View style={styles.header}>
              <View style={styles.headerCopy}>
                <Text
                  accessibilityRole="header"
                  style={[styles.title, { color: t.ink }]}
                >
                  react-native-scc
                </Text>
                <Text style={[styles.subtitle, { color: t.sub }]}>
                  Rust-powered key-value storage · example
                </Text>
              </View>
              <View
                accessibilityLabel={`Self-test status: ${statusLabel}`}
                accessible
                style={styles.status}
              >
                <View
                  style={[styles.statusDot, { backgroundColor: statusColor }]}
                />
                <Text style={[styles.statusLabel, { color: statusColor }]}>
                  {statusLabel}
                </Text>
              </View>
            </View>

            <Demos t={t} />

            <SelfTestSection
              error={selfTestError}
              onRun={() => void executeSelfTest(false)}
              results={selfTestResults}
              running={selfTestRunning}
              t={t}
            />

            <BenchmarkSection
              error={benchmarkError}
              onRun={() => void executeBenchmark()}
              progress={benchmarkProgress}
              report={benchmarkReport}
              running={benchmarkRunning}
              t={t}
            />
          </View>
        </ScrollView>
      </SafeAreaView>
    </SafeAreaProvider>
  )
}
