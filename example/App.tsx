import { useAtom } from 'jotai'
import { useEffect, useRef, useState } from 'react'
import {
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  useColorScheme,
  View,
} from 'react-native'
import { createMMKV } from 'react-native-mmkv'
import {
  SafeAreaProvider,
  SafeAreaView,
} from 'react-native-safe-area-context'
import { createKV, useKVNumber, useKVSelector } from 'react-native-scc-storage'
import { atomWithKV } from 'react-native-scc-storage/jotai'
import { sccStateStorage } from 'react-native-scc-storage/zustand'
import { create } from 'zustand'
import { createJSONStorage, persist } from 'zustand/middleware'

interface Result {
  name: string
  ok: boolean
  detail: string
}

interface BenchCase {
  name: string
  scc: number
  mmkv: number
}

const kv = createKV({ id: 'example' })
// Dedicated instances keep the benchmark clean: no hook/selftest listeners
// attached, so neither side pays change-notification dispatch costs.
const kvBench = createKV({ id: 'bench_scc' })
const mmkv = createMMKV({ id: 'bench' })

async function runSelfTest(): Promise<Result[]> {
  const results: Result[] = []
  const check = (name: string, ok: boolean, detail = '') => {
    results.push({ name, ok, detail })
  }

  const launches = (kv.getNumber('launches') ?? 0) + 1
  kv.set('launches', launches)
  console.log(`SCC_LAUNCHES=${launches}`)
  check('persistence: launch counter', launches >= 1, `launch #${launches}`)

  kv.set('str', 'hello')
  kv.set('num', 42.5)
  kv.set('bool', true)
  kv.setJSON('json', { nested: [1, 2, 3] })
  const buf = new Uint8Array([9, 8, 7]).buffer
  kv.set('buf', buf)

  check('sync string', kv.getString('str') === 'hello')
  check('sync number', kv.getNumber('num') === 42.5)
  check('sync boolean', kv.getBoolean('bool') === true)
  check('sync json', kv.getJSON<{ nested: number[] }>('json')?.nested[2] === 3)
  const back = kv.getBuffer('buf')
  check('sync buffer', back !== undefined && new Uint8Array(back)[1] === 8)
  check('type mismatch is undefined', kv.getString('num') === undefined)
  check(
    'contains / delete',
    kv.contains('str') && kv.delete('str') && !kv.contains('str')
  )
  check('keys', kv.getAllKeys().includes('num'))

  await kv.setAsync('astr', 'async-hello')
  const astr = await kv.getStringAsync('astr')
  check('async roundtrip', astr === 'async-hello')
  const missing = await kv.getNumberAsync('does-not-exist')
  check('async missing is undefined', missing === undefined)
  await kv.setJSONAsync('ajson', { ok: true })
  const ajson = await kv.getJSONAsync<{ ok: boolean }>('ajson')
  check('async json', ajson?.ok === true)
  await kv.flushAsync()
  check('async flush', true)

  const listenerEvent = new Promise<string | null>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('listener timeout')), 2000)
    const sub = kv.addOnValueChangedListener((key) => {
      if (key === 'cross_key') {
        clearTimeout(timer)
        sub.remove()
        resolve(key)
      }
    })
  })
  const kv2 = createKV({ id: 'example' })
  kv2.set('cross_key', 'from-second-handle')
  try {
    check(
      'native listener (cross-handle)',
      (await listenerEvent) === 'cross_key'
    )
  } catch (e) {
    check('native listener (cross-handle)', false, String(e))
  }

  kv.set('ttl_key', 'temporary', { ttlMs: 400 })
  check('ttl: readable before expiry', kv.getString('ttl_key') === 'temporary')
  await new Promise((resolve) => setTimeout(resolve, 600))
  check('ttl: expired after deadline', kv.getString('ttl_key') === undefined)

  const vault = createKV({ id: 'vault', encryptionKey: 'example-passphrase' })
  const vaultLaunches = (vault.getNumber('launches') ?? 0) + 1
  vault.set('launches', vaultLaunches)
  vault.set('secret', 'classified')
  check(
    'encrypted vault roundtrip',
    vault.getString('secret') === 'classified',
    `launch #${vaultLaunches}`
  )
  vault.flush()

  kv.set('tx_drop', 'x')
  const txResult = kv.transaction((tx) => {
    const next = (tx.getNumber('tx_counter') ?? 0) + 1
    tx.set('tx_counter', next)
    tx.setJSON('tx_meta', { next })
    tx.delete('tx_drop')
    return next
  })
  check(
    'transaction: atomic batch commit',
    kv.getNumber('tx_counter') === txResult &&
      kv.getJSON<{ next: number }>('tx_meta')?.next === txResult &&
      !kv.contains('tx_drop'),
    `commit #${txResult}`
  )

  const profile = kv.namespace('profile')
  profile.clearAll()
  profile.set('name', 'Ada')
  profile.setJSON('prefs', { theme: 'dark' })
  check(
    'namespace: scoped keys',
    profile.getString('name') === 'Ada' &&
      profile.size === 2 &&
      kv.getString('profile:name') === 'Ada' &&
      profile.getAllKeys().sort().join(',') === 'name,prefs'
  )
  check(
    'namespace: scoped clearAll',
    profile.clearAll() === 2 && !kv.contains('profile:name')
  )

  kv.setJSON('obs_settings', { theme: 'dark', volume: 1 })
  const observed = new Promise<string | undefined>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('observe timeout')), 2000)
    const sub = kv.observeJSON<{ theme: string }, string | undefined>(
      'obs_settings',
      (s) => s?.theme,
      (theme) => {
        if (theme === 'light') {
          clearTimeout(timer)
          sub.remove()
          resolve(theme)
        }
      }
    )
    kv.setJSON('obs_settings', { theme: 'light', volume: 1 })
  })
  try {
    check('observeJSON: selected change emits', (await observed) === 'light')
  } catch (e) {
    check('observeJSON: selected change emits', false, String(e))
  }

  const cache = createKV({
    id: 'evict-demo',
    persistence: 'none',
    maxEntries: 8,
    ttlSweepIntervalMs: 50,
  })
  for (let i = 0; i < 32; i++) cache.set(`item_${i}`, i)
  await new Promise((resolve) => setTimeout(resolve, 400))
  check(
    'eviction: in-memory maxEntries cap',
    cache.size <= 8,
    `${cache.size}/32 kept`
  )

  kv.flush()
  return results
}

const payload = 'x'.repeat(64)
const payload16 = 'x'.repeat(16)

function measure(fn: () => void, iters = 100_000): number {
  for (let i = 0; i < 1_000; i++) fn()
  const t0 = performance.now()
  for (let i = 0; i < iters; i++) fn()
  return ((performance.now() - t0) * 1e6) / iters
}

const batchKeys = Array.from({ length: 100 }, (_, i) => `bm_${i}`)
const batchEntries: Record<string, string> = Object.fromEntries(
  batchKeys.map((k) => [k, payload16])
)

function runBench(): BenchCase[] {
  const cases: Array<[string, () => void, () => void]> = [
    ['set_string64', () => kvBench.set('b_s', payload), () => mmkv.set('b_s', payload)],
    ['get_string64', () => kvBench.getString('b_s'), () => mmkv.getString('b_s')],
    ['set_string16', () => kvBench.set('b_s16', payload16), () => mmkv.set('b_s16', payload16)],
    ['get_string16', () => kvBench.getString('b_s16'), () => mmkv.getString('b_s16')],
    ['set_number', () => kvBench.set('b_n', 42.5), () => mmkv.set('b_n', 42.5)],
    ['get_number', () => kvBench.getNumber('b_n'), () => mmkv.getNumber('b_n')],
    ['get_miss', () => kvBench.getString('b_missing'), () => mmkv.getString('b_missing')],
  ]
  const results: BenchCase[] = []
  for (const [name, sccFn, mmkvFn] of cases) {
    const scc = measure(sccFn)
    const mm = measure(mmkvFn)
    console.log(
      `SCC_BENCH case=${name} scc=${scc.toFixed(0)}ns mmkv=${mm.toFixed(0)}ns`
    )
    results.push({ name, scc, mmkv: mm })
  }

  const sccBatch = measure(() => kvBench.setMany(batchEntries), 1_000) / 100
  const mmkvLoop =
    measure(() => {
      for (const k of batchKeys) mmkv.set(k, payload16)
    }, 1_000) / 100
  console.log(
    `SCC_BENCH case=set100x16_perkey scc=${sccBatch.toFixed(0)}ns mmkv=${mmkvLoop.toFixed(0)}ns`
  )
  results.push({ name: 'set100x16_perkey', scc: sccBatch, mmkv: mmkvLoop })

  console.log('SCC_BENCH_DONE')
  // Release builds drop console.log, so persist results for host-side readout.
  kv.setJSON('last_bench', results)
  kv.flush()
  return results
}

const mono = Platform.select({ ios: 'Menlo', default: 'monospace' })

const palettes = {
  light: {
    bg: '#F6F5F2',
    card: '#FFFFFF',
    border: 'rgba(28,27,24,0.08)',
    ink: '#1C1B18',
    sub: '#6B675E',
    faint: '#98948A',
    accent: '#C2410C',
    accentSoft: 'rgba(194,65,12,0.10)',
    good: '#15803D',
    goodSoft: 'rgba(21,128,61,0.10)',
    bad: '#B91C1C',
    badSoft: 'rgba(185,28,28,0.10)',
    track: 'rgba(28,27,24,0.06)',
    mmkv: '#8A8E98',
  },
  dark: {
    bg: '#131211',
    card: '#1D1B19',
    border: 'rgba(242,240,235,0.09)',
    ink: '#F1EFE9',
    sub: '#A6A196',
    faint: '#7C786E',
    accent: '#E8632C',
    accentSoft: 'rgba(232,99,44,0.16)',
    good: '#4ADE80',
    goodSoft: 'rgba(74,222,128,0.13)',
    bad: '#F87171',
    badSoft: 'rgba(248,113,113,0.13)',
    track: 'rgba(242,240,235,0.08)',
    mmkv: '#6E7480',
  },
}

type Palette = (typeof palettes)['light']

function PillButton({
  label,
  onPress,
  t,
}: {
  label: string
  onPress: () => void
  t: Palette
}) {
  return (
    <Pressable
      onPress={onPress}
      style={({ pressed }) => [
        styles.pill,
        { backgroundColor: t.accentSoft, opacity: pressed ? 0.6 : 1 },
      ]}
    >
      <Text style={[styles.pillLabel, { color: t.accent }]}>{label}</Text>
    </Pressable>
  )
}

function Card({
  title,
  caption,
  t,
  children,
}: {
  title: string
  caption?: string
  t: Palette
  children: React.ReactNode
}) {
  return (
    <View style={[styles.card, { backgroundColor: t.card, borderColor: t.border }]}>
      <View style={styles.cardHeader}>
        <Text style={[styles.eyebrow, { color: t.faint }]}>{title}</Text>
        {caption !== undefined && (
          <Text style={[styles.caption, { color: t.faint }]}>{caption}</Text>
        )}
      </View>
      {children}
    </View>
  )
}

function CounterRow({
  label,
  value,
  onAdd,
  t,
}: {
  label: string
  value: number
  onAdd: () => void
  t: Palette
}) {
  return (
    <View style={styles.counterRow}>
      <Text style={[styles.counterLabel, { color: t.sub }]}>{label}</Text>
      <View style={styles.counterRight}>
        <Text style={[styles.counterValue, { color: t.ink }]}>{value}</Text>
        <PillButton label="+1" onPress={onAdd} t={t} />
      </View>
    </View>
  )
}

function HookDemo({ t }: { t: Palette }) {
  const [count, setCount] = useKVNumber('hook_counter', kv)
  return (
    <CounterRow
      label="useKVNumber"
      value={count ?? 0}
      onAdd={() => setCount((count ?? 0) + 1)}
      t={t}
    />
  )
}

function DemoRow({
  label,
  hint,
  value,
  t,
  children,
}: {
  label: string
  hint?: string
  value?: string
  t: Palette
  children: React.ReactNode
}) {
  return (
    <View style={styles.demoRow}>
      <View style={styles.demoTop}>
        <View style={styles.demoLabelWrap}>
          <Text
            style={[styles.counterLabel, { color: t.sub }]}
            numberOfLines={1}
          >
            {label}
          </Text>
          {hint !== undefined && (
            <Text
              style={[styles.caption, { color: t.faint }]}
              numberOfLines={1}
            >
              {hint}
            </Text>
          )}
        </View>
        {value !== undefined && (
          <Text style={[styles.counterValue, { color: t.ink }]} numberOfLines={1}>
            {value}
          </Text>
        )}
      </View>
      <View style={styles.demoActions}>{children}</View>
    </View>
  )
}

function TransactionDemo({ t }: { t: Palette }) {
  const [a] = useKVNumber('bal_a', kv)
  const [b] = useKVNumber('bal_b', kv)
  const from = a ?? 100
  const to = b ?? 0
  const transfer = () => {
    kv.transaction((tx) => {
      const src = tx.getNumber('bal_a') ?? 100
      const dst = tx.getNumber('bal_b') ?? 0
      if (src >= 10) {
        tx.set('bal_a', src - 10)
        tx.set('bal_b', dst + 10)
      } else {
        tx.set('bal_a', 100)
        tx.set('bal_b', 0)
      }
    })
  }
  return (
    <DemoRow
      label="transaction"
      hint="two writes, one atomic batch"
      value={`${from} → ${to}`}
      t={t}
    >
      <PillButton
        label={from >= 10 ? 'move 10' : 'reset'}
        onPress={transfer}
        t={t}
      />
    </DemoRow>
  )
}

const demoNs = kv.namespace('demo')

function NamespaceDemo({ t }: { t: Palette }) {
  const [count, setCount] = useKVNumber('ns_counter', demoNs)
  return (
    <DemoRow
      label="namespace('demo')"
      hint="stored as demo:ns_counter"
      value={String(count ?? 0)}
      t={t}
    >
      <PillButton label="+1" onPress={() => setCount((count ?? 0) + 1)} t={t} />
    </DemoRow>
  )
}

interface UiPrefs {
  theme?: string
  taps?: number
}

function SelectorDemo({ t }: { t: Palette }) {
  const theme = useKVSelector<UiPrefs, string>(
    'ui_prefs',
    (p) => p?.theme ?? 'dark',
    kv
  )
  const renders = useRef(0)
  renders.current += 1
  const patch = (change: Partial<UiPrefs>) => {
    kv.setJSON('ui_prefs', { ...kv.getJSON<UiPrefs>('ui_prefs'), ...change })
  }
  return (
    <DemoRow
      label="useKVSelector"
      hint={`selects .theme · ${renders.current} renders`}
      value={theme}
      t={t}
    >
      <PillButton
        label="toggle theme"
        onPress={() => patch({ theme: theme === 'dark' ? 'light' : 'dark' })}
        t={t}
      />
      <PillButton
        label="bump other"
        onPress={() =>
          patch({ taps: (kv.getJSON<UiPrefs>('ui_prefs')?.taps ?? 0) + 1 })
        }
        t={t}
      />
    </DemoRow>
  )
}

const useBearStore = create(
  persist<{ bears: number; add: () => void }>(
    (set) => ({
      bears: 0,
      add: () => set((s) => ({ bears: s.bears + 1 })),
    }),
    {
      name: 'zustand_bears',
      storage: createJSONStorage(() => sccStateStorage(kv)),
    }
  )
)

function ZustandDemo({ t }: { t: Palette }) {
  const bears = useBearStore((s) => s.bears)
  const add = useBearStore((s) => s.add)
  return <CounterRow label="zustand persist" value={bears} onAdd={add} t={t} />
}

const jotaiCounterAtom = atomWithKV('jotai_counter', 0, kv)

function JotaiDemo({ t }: { t: Palette }) {
  const [count, setCount] = useAtom(jotaiCounterAtom)
  return (
    <CounterRow
      label="jotai atomWithKV"
      value={count}
      onAdd={() => setCount((c) => c + 1)}
      t={t}
    />
  )
}

function formatNs(ns: number): string {
  return ns >= 1000 ? `${(ns / 1000).toFixed(1)} µs` : `${ns.toFixed(0)} ns`
}

function BenchRow({ result, t }: { result: BenchCase; t: Palette }) {
  const max = Math.max(result.scc, result.mmkv)
  const faster = result.mmkv >= result.scc
  const ratio = faster ? result.mmkv / result.scc : result.scc / result.mmkv
  const chipColor = faster ? t.good : t.bad
  const chipBg = faster ? t.goodSoft : t.badSoft
  return (
    <View style={styles.benchRow}>
      <View style={styles.benchHeader}>
        <Text style={[styles.benchName, { color: t.ink }]}>{result.name}</Text>
        <View style={[styles.chip, { backgroundColor: chipBg }]}>
          <Text style={[styles.chipLabel, { color: chipColor }]}>
            {ratio.toFixed(1)}× {faster ? 'faster' : 'slower'}
          </Text>
        </View>
      </View>
      {(
        [
          ['scc', result.scc, t.accent],
          ['mmkv', result.mmkv, t.mmkv],
        ] as const
      ).map(([series, value, color]) => (
        <View key={series} style={styles.barRow}>
          <Text style={[styles.barLabel, { color: t.faint }]}>{series}</Text>
          <View style={[styles.barTrack, { backgroundColor: t.track }]}>
            <View
              style={[
                styles.barFill,
                { backgroundColor: color, width: `${(value / max) * 100}%` },
              ]}
            />
          </View>
          <Text style={[styles.barValue, { color: t.sub }]}>
            {formatNs(value)}
          </Text>
        </View>
      ))}
    </View>
  )
}

export default function App() {
  const scheme = useColorScheme()
  const t = palettes[scheme === 'dark' ? 'dark' : 'light']
  const [results, setResults] = useState<Result[]>([])
  const [bench, setBench] = useState<BenchCase[]>([])
  const [error, setError] = useState<string>()

  useEffect(() => {
    runSelfTest()
      .then((r) => {
        setResults(r)
        const failed = r.filter((x) => !x.ok)
        if (failed.length === 0) console.log('SCC_SELFTEST_OK')
        else
          console.log(
            `SCC_SELFTEST_FAIL: ${failed.map((f) => f.name).join(', ')}`
          )
        setBench(runBench())
      })
      .catch((e) => {
        setError(String(e))
        console.log(`SCC_SELFTEST_FAIL: ${e}`)
      })
  }, [])

  const passed = results.filter((r) => r.ok).length
  const failed = results.length - passed
  const running = results.length === 0 && error === undefined
  const statusColor = running ? t.faint : failed === 0 ? t.good : t.bad
  const statusLabel = running
    ? 'running…'
    : failed === 0
      ? 'all passing'
      : `${failed} failing`

  return (
    <SafeAreaProvider>
      <SafeAreaView
        style={[styles.screen, { backgroundColor: t.bg }]}
        edges={['top', 'left', 'right']}
      >
        <ScrollView contentContainerStyle={styles.scroll}>
          <View style={styles.header}>
            <View>
              <Text style={[styles.title, { color: t.ink }]}>
                react-native-scc
              </Text>
              <Text style={[styles.subtitle, { color: t.sub }]}>
                Rust-powered key-value storage · example
              </Text>
            </View>
            <View style={styles.status}>
              <View style={[styles.statusDot, { backgroundColor: statusColor }]} />
              <Text style={[styles.statusLabel, { color: statusColor }]}>
                {statusLabel}
              </Text>
            </View>
          </View>

          <Card title="LIVE STATE" caption="persisted across restarts" t={t}>
            <HookDemo t={t} />
            <View style={[styles.divider, { backgroundColor: t.border }]} />
            <ZustandDemo t={t} />
            <View style={[styles.divider, { backgroundColor: t.border }]} />
            <JotaiDemo t={t} />
          </Card>

          <Card title="TRANSACTIONS · NAMESPACES · SELECTORS" t={t}>
            <TransactionDemo t={t} />
            <View style={[styles.divider, { backgroundColor: t.border }]} />
            <NamespaceDemo t={t} />
            <View style={[styles.divider, { backgroundColor: t.border }]} />
            <SelectorDemo t={t} />
          </Card>

          <Card
            title="SELF-TEST"
            caption={running ? undefined : `${passed}/${results.length} passed`}
            t={t}
          >
            {error !== undefined && (
              <Text style={[styles.failText, { color: t.bad }]}>{error}</Text>
            )}
            {running && error === undefined && (
              <Text style={[styles.caption, { color: t.faint }]}>
                exercising sync, async, ttl and encryption paths…
              </Text>
            )}
            {results.map((r) => (
              <View key={r.name} style={styles.testRow}>
                <Text
                  style={[styles.testTick, { color: r.ok ? t.good : t.bad }]}
                >
                  {r.ok ? '✓' : '✕'}
                </Text>
                <Text
                  style={[styles.testName, { color: r.ok ? t.sub : t.bad }]}
                  numberOfLines={1}
                >
                  {r.name}
                </Text>
                {r.detail !== '' && (
                  <Text style={[styles.testDetail, { color: t.faint }]}>
                    {r.detail}
                  </Text>
                )}
              </View>
            ))}
          </Card>

          <Card title="BENCHMARK · VS MMKV" caption="lower is better" t={t}>
            {bench.length === 0 ? (
              <Text style={[styles.caption, { color: t.faint }]}>
                waiting for self-test…
              </Text>
            ) : (
              <>
                {bench.map((b) => (
                  <BenchRow key={b.name} result={b} t={t} />
                ))}
                <View style={styles.benchActions}>
                  <PillButton
                    label="Run benchmark again"
                    onPress={() => setBench(runBench())}
                    t={t}
                  />
                </View>
              </>
            )}
          </Card>
        </ScrollView>
      </SafeAreaView>
    </SafeAreaProvider>
  )
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  scroll: { padding: 16, paddingBottom: 40, gap: 12 },
  header: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'flex-start',
    paddingHorizontal: 4,
    paddingTop: 8,
    paddingBottom: 4,
  },
  title: { fontSize: 22, fontWeight: '700', letterSpacing: -0.4 },
  subtitle: { fontSize: 13, marginTop: 2 },
  status: { flexDirection: 'row', alignItems: 'center', gap: 6, paddingTop: 6 },
  statusDot: { width: 8, height: 8, borderRadius: 4 },
  statusLabel: { fontSize: 12, fontWeight: '600' },
  card: {
    borderRadius: 14,
    borderWidth: StyleSheet.hairlineWidth,
    padding: 16,
  },
  cardHeader: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 10,
  },
  eyebrow: { fontSize: 11, fontWeight: '700', letterSpacing: 1.2 },
  caption: { fontSize: 12 },
  divider: { height: StyleSheet.hairlineWidth, marginVertical: 10 },
  counterRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  counterLabel: { fontSize: 14 },
  counterRight: { flexDirection: 'row', alignItems: 'center', gap: 12 },
  counterValue: {
    fontFamily: mono,
    fontSize: 17,
    fontWeight: '600',
    minWidth: 36,
    textAlign: 'right',
  },
  demoRow: { gap: 10 },
  demoTop: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 12,
  },
  demoLabelWrap: { flexShrink: 1, gap: 2 },
  demoActions: { flexDirection: 'row', flexWrap: 'wrap', gap: 8 },
  pill: {
    borderRadius: 999,
    paddingHorizontal: 14,
    paddingVertical: 7,
  },
  pillLabel: { fontSize: 13, fontWeight: '600' },
  testRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingVertical: 3.5,
  },
  testTick: { fontFamily: mono, fontSize: 13, width: 16 },
  testName: { fontSize: 13, flexShrink: 1 },
  testDetail: { fontSize: 12, fontFamily: mono, marginLeft: 'auto' },
  failText: { fontSize: 13, marginBottom: 6 },
  benchRow: { marginBottom: 14 },
  benchHeader: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 6,
  },
  benchName: { fontFamily: mono, fontSize: 13, fontWeight: '600' },
  chip: { borderRadius: 999, paddingHorizontal: 8, paddingVertical: 2.5 },
  chipLabel: { fontSize: 11, fontWeight: '700' },
  barRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    marginVertical: 2.5,
  },
  barLabel: { fontFamily: mono, fontSize: 11, width: 36 },
  barTrack: {
    flex: 1,
    height: 8,
    borderRadius: 4,
    overflow: 'hidden',
  },
  barFill: { height: 8, borderRadius: 4 },
  barValue: {
    fontFamily: mono,
    fontSize: 12,
    width: 64,
    textAlign: 'right',
  },
  benchActions: { alignItems: 'center', marginTop: 4 },
})
