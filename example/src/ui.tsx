import type { ReactNode } from 'react'
import { Pressable, Text, View } from 'react-native'
import { type Palette, styles } from './theme'

export function PillButton({
  label,
  onPress,
  t,
  disabled = false,
  accessibilityLabel,
  accessibilityHint,
}: {
  label: string
  onPress: () => void
  t: Palette
  disabled?: boolean
  accessibilityLabel?: string
  accessibilityHint?: string
}) {
  return (
    <Pressable
      accessibilityHint={accessibilityHint}
      accessibilityLabel={accessibilityLabel}
      accessibilityRole="button"
      accessibilityState={{ disabled }}
      disabled={disabled}
      hitSlop={4}
      onPress={onPress}
      style={({ pressed }) => [
        styles.pill,
        {
          backgroundColor: t.accentSoft,
          opacity: disabled ? 0.45 : pressed ? 0.65 : 1,
        },
      ]}
    >
      <Text style={[styles.pillLabel, { color: t.accent }]}>{label}</Text>
    </Pressable>
  )
}

export function Card({
  title,
  caption,
  t,
  children,
}: {
  title: string
  caption?: string
  t: Palette
  children: ReactNode
}) {
  return (
    <View
      style={[styles.card, { backgroundColor: t.card, borderColor: t.border }]}
    >
      <View style={styles.cardHeader}>
        <Text
          accessibilityRole="header"
          style={[styles.eyebrow, { color: t.faint }]}
        >
          {title}
        </Text>
        {caption !== undefined && (
          <Text style={[styles.caption, { color: t.faint }]}>{caption}</Text>
        )}
      </View>
      {children}
    </View>
  )
}

export function Divider({ t }: { t: Palette }) {
  return <View style={[styles.divider, { backgroundColor: t.border }]} />
}

export function CounterRow({
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
        <Text
          accessibilityLabel={`${label}: ${value}`}
          style={[styles.counterValue, { color: t.ink }]}
        >
          {value}
        </Text>
        <PillButton
          accessibilityLabel={`Increment ${label}`}
          label="+1"
          onPress={onAdd}
          t={t}
        />
      </View>
    </View>
  )
}

export function DemoRow({
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
  children: ReactNode
}) {
  return (
    <View style={styles.demoRow}>
      <View style={styles.demoTop}>
        <View style={styles.demoLabelWrap}>
          <Text style={[styles.counterLabel, { color: t.sub }]}>{label}</Text>
          {hint !== undefined && (
            <Text style={[styles.caption, { color: t.faint }]}>{hint}</Text>
          )}
        </View>
        {value !== undefined && (
          <Text style={[styles.counterValue, { color: t.ink }]}>{value}</Text>
        )}
      </View>
      <View style={styles.demoActions}>{children}</View>
    </View>
  )
}
