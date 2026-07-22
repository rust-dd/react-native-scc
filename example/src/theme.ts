import { Platform, StyleSheet } from 'react-native'

export interface Palette {
  bg: string
  card: string
  border: string
  ink: string
  sub: string
  faint: string
  accent: string
  accentSoft: string
  good: string
  goodSoft: string
  bad: string
  badSoft: string
  track: string
  mmkv: string
}

export const palettes: Record<'light' | 'dark', Palette> = {
  light: {
    bg: '#F6F5F2',
    card: '#FFFFFF',
    border: 'rgba(28,27,24,0.08)',
    ink: '#1C1B18',
    sub: '#6B675E',
    faint: '#747068',
    accent: '#C2410C',
    accentSoft: 'rgba(194,65,12,0.10)',
    good: '#15803D',
    goodSoft: 'rgba(21,128,61,0.10)',
    bad: '#B91C1C',
    badSoft: 'rgba(185,28,28,0.10)',
    track: 'rgba(28,27,24,0.06)',
    mmkv: '#747985',
  },
  dark: {
    bg: '#131211',
    card: '#1D1B19',
    border: 'rgba(242,240,235,0.09)',
    ink: '#F1EFE9',
    sub: '#A6A196',
    faint: '#969187',
    accent: '#E8632C',
    accentSoft: 'rgba(232,99,44,0.16)',
    good: '#4ADE80',
    goodSoft: 'rgba(74,222,128,0.13)',
    bad: '#F87171',
    badSoft: 'rgba(248,113,113,0.13)',
    track: 'rgba(242,240,235,0.08)',
    mmkv: '#8C929E',
  },
}

export const mono = Platform.select({ ios: 'Menlo', default: 'monospace' })

export const styles = StyleSheet.create({
  screen: { flex: 1 },
  scroll: { padding: 16, paddingBottom: 24 },
  content: {
    width: '100%',
    maxWidth: 760,
    alignSelf: 'center',
    gap: 12,
  },
  header: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    justifyContent: 'space-between',
    alignItems: 'flex-start',
    gap: 12,
    paddingHorizontal: 4,
    paddingTop: 8,
    paddingBottom: 4,
  },
  headerCopy: { flexShrink: 1 },
  title: { fontSize: 22, fontWeight: '700', letterSpacing: -0.4 },
  subtitle: { fontSize: 13, marginTop: 2 },
  status: { flexDirection: 'row', alignItems: 'center', gap: 6, minHeight: 32 },
  statusDot: { width: 8, height: 8, borderRadius: 4 },
  statusLabel: { fontSize: 12, fontWeight: '600' },
  card: {
    borderRadius: 14,
    borderWidth: StyleSheet.hairlineWidth,
    padding: 16,
  },
  cardHeader: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 6,
    marginBottom: 10,
  },
  eyebrow: { fontSize: 11, fontWeight: '700', letterSpacing: 1.2 },
  caption: { fontSize: 12, lineHeight: 17 },
  divider: { height: StyleSheet.hairlineWidth, marginVertical: 10 },
  counterRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 12,
  },
  counterLabel: { fontSize: 14, flexShrink: 1 },
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
    minWidth: 44,
    minHeight: 44,
    borderRadius: 999,
    paddingHorizontal: 14,
    paddingVertical: 9,
    alignItems: 'center',
    justifyContent: 'center',
  },
  pillLabel: { fontSize: 13, fontWeight: '600', textAlign: 'center' },
  testRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    minHeight: 28,
  },
  testTick: { fontFamily: mono, fontSize: 13, width: 16 },
  testName: { fontSize: 13, flexShrink: 1 },
  testDetail: {
    fontSize: 12,
    fontFamily: mono,
    marginLeft: 'auto',
    flexShrink: 1,
    textAlign: 'right',
  },
  sectionMessage: { fontSize: 13, lineHeight: 19 },
  sectionActions: { alignItems: 'center', marginTop: 12 },
  warning: { borderRadius: 10, padding: 10, marginBottom: 12 },
  warningText: { fontSize: 12, lineHeight: 17 },
  metadata: { fontSize: 11, lineHeight: 16, marginBottom: 12 },
  progress: { marginBottom: 12, gap: 5 },
  progressTrack: { height: 6, borderRadius: 3, overflow: 'hidden' },
  progressFill: { height: 6, borderRadius: 3 },
  benchRow: { marginBottom: 16 },
  benchHeader: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'flex-start',
    gap: 8,
    marginBottom: 7,
  },
  benchTitleWrap: { flex: 1, gap: 2 },
  benchName: { fontFamily: mono, fontSize: 13, fontWeight: '600' },
  benchDetail: { fontSize: 11, lineHeight: 15 },
  chip: { borderRadius: 999, paddingHorizontal: 8, paddingVertical: 3 },
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
})
