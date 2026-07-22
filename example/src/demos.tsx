import { useAtom } from 'jotai'
import { memo, useRef } from 'react'
import { Text } from 'react-native'
import { useKVNumber, useKVSelector } from 'react-native-scc-storage'
import { atomWithKV } from 'react-native-scc-storage/jotai'
import { sccStateStorage } from 'react-native-scc-storage/zustand'
import { create } from 'zustand'
import { createJSONStorage, persist } from 'zustand/middleware'
import { kv } from './storage'
import { type Palette, styles } from './theme'
import { Card, CounterRow, DemoRow, Divider, PillButton } from './ui'

function HookDemo({ t }: { t: Palette }) {
  const [count, setCount] = useKVNumber('hook_counter', kv)
  return (
    <CounterRow
      label="useKVNumber"
      onAdd={() => setCount((count ?? 0) + 1)}
      t={t}
      value={count ?? 0}
    />
  )
}

function TransactionDemo({ t }: { t: Palette }) {
  const [source] = useKVNumber('bal_a', kv)
  const [destination] = useKVNumber('bal_b', kv)
  const from = source ?? 100
  const to = destination ?? 0

  const transfer = () => {
    kv.transaction((transaction) => {
      const currentSource = transaction.getNumber('bal_a') ?? 100
      const currentDestination = transaction.getNumber('bal_b') ?? 0
      if (currentSource >= 10) {
        transaction.set('bal_a', currentSource - 10)
        transaction.set('bal_b', currentDestination + 10)
      } else {
        transaction.set('bal_a', 100)
        transaction.set('bal_b', 0)
      }
    })
  }

  return (
    <DemoRow
      hint="two writes, one atomic batch"
      label="transaction"
      t={t}
      value={`${from} → ${to}`}
    >
      <PillButton
        label={from >= 10 ? 'move 10' : 'reset'}
        onPress={transfer}
        t={t}
      />
    </DemoRow>
  )
}

const demoNamespace = kv.namespace('demo')

function NamespaceDemo({ t }: { t: Palette }) {
  const [count, setCount] = useKVNumber('ns_counter', demoNamespace)
  return (
    <DemoRow
      hint="stored as demo:ns_counter"
      label="namespace('demo')"
      t={t}
      value={String(count ?? 0)}
    >
      <PillButton label="+1" onPress={() => setCount((count ?? 0) + 1)} t={t} />
    </DemoRow>
  )
}

interface UiPreferences {
  theme?: string
  taps?: number
}

function SelectorDemo({ t }: { t: Palette }) {
  const theme = useKVSelector<UiPreferences, string>(
    'ui_prefs',
    (preferences) => preferences?.theme ?? 'dark',
    kv
  )
  const renders = useRef(0)
  renders.current += 1

  const patch = (change: Partial<UiPreferences>) => {
    kv.setJSON('ui_prefs', {
      ...kv.getJSON<UiPreferences>('ui_prefs'),
      ...change,
    })
  }

  return (
    <DemoRow
      hint={`selects .theme · ${renders.current} renders`}
      label="useKVSelector"
      t={t}
      value={theme}
    >
      <PillButton
        label="toggle theme"
        onPress={() => patch({ theme: theme === 'dark' ? 'light' : 'dark' })}
        t={t}
      />
      <PillButton
        label="bump other"
        onPress={() =>
          patch({
            taps: (kv.getJSON<UiPreferences>('ui_prefs')?.taps ?? 0) + 1,
          })
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
      add: () => set((state) => ({ bears: state.bears + 1 })),
    }),
    {
      name: 'zustand_bears',
      storage: createJSONStorage(() => sccStateStorage(kv)),
    }
  )
)

function ZustandDemo({ t }: { t: Palette }) {
  const bears = useBearStore((state) => state.bears)
  const add = useBearStore((state) => state.add)
  return <CounterRow label="zustand persist" onAdd={add} t={t} value={bears} />
}

const jotaiCounterAtom = atomWithKV('jotai_counter', 0, kv)

function JotaiDemo({ t }: { t: Palette }) {
  const [count, setCount] = useAtom(jotaiCounterAtom)
  return (
    <CounterRow
      label="jotai atomWithKV"
      onAdd={() => setCount((current) => current + 1)}
      t={t}
      value={count}
    />
  )
}

export const Demos = memo(function Demos({ t }: { t: Palette }) {
  return (
    <>
      <Card caption="persisted across restarts" t={t} title="LIVE STATE">
        <HookDemo t={t} />
        <Divider t={t} />
        <ZustandDemo t={t} />
        <Divider t={t} />
        <JotaiDemo t={t} />
      </Card>

      <Card t={t} title="TRANSACTIONS · NAMESPACES · SELECTORS">
        <TransactionDemo t={t} />
        <Divider t={t} />
        <NamespaceDemo t={t} />
        <Divider t={t} />
        <SelectorDemo t={t} />
      </Card>

      <Text style={[styles.caption, { color: t.faint, textAlign: 'center' }]}>
        Adapter state and self-test data survive native app restarts.
      </Text>
    </>
  )
})
