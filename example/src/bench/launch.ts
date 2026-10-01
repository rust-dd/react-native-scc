import { Platform, Settings } from 'react-native'

function launchArgument(key: string): string | undefined {
  if (Platform.OS !== 'ios') return undefined
  const value: unknown = Settings.get(key)
  return value === undefined || value === null ? undefined : String(value)
}

export function shouldAutorunBenchmark(): boolean {
  return (
    launchArgument('sccBenchAutorun') === '1' ||
    process.env.EXPO_PUBLIC_SCC_AUTORUN_BENCHMARK === '1'
  )
}

export function benchmarkProfileName(): string {
  return (
    launchArgument('sccBenchProfile') ??
    process.env.EXPO_PUBLIC_SCC_BENCH_PROFILE ??
    'full'
  )
}

export interface ProfileRequest {
  caseId: string
  lib: 'scc' | 'mmkv'
  seconds: number
}

export function profileRequest(): ProfileRequest | undefined {
  const caseId = launchArgument('sccProfileCase')
  if (caseId === undefined) return undefined
  return {
    caseId,
    lib: launchArgument('sccProfileLib') === 'mmkv' ? 'mmkv' : 'scc',
    seconds: Number(launchArgument('sccProfileSeconds') ?? '15'),
  }
}
