import { createMMKV } from 'react-native-mmkv'
import { createKV } from 'react-native-scc-storage'

export const kv = createKV({ id: 'example' })

export const kvCrossHandle = createKV({ id: 'example' })

export const createSccBenchmarkStore = () =>
  createKV({ id: 'bench_scc', recreate: true })

export const mmkvBench = createMMKV({
  id: 'bench',
  compareBeforeSet: false,
})
