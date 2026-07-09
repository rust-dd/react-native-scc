import type { HybridObject } from 'react-native-nitro-modules'

export interface SccKvInstance
  extends HybridObject<{ ios: 'c++'; android: 'c++' }> {
  setString(key: string, value: string): void
  setNumber(key: string, value: number): void
  setBoolean(key: string, value: boolean): void
  setBuffer(key: string, value: ArrayBuffer): void
  setJson(key: string, json: string): void
  getString(key: string): string | undefined
  getNumber(key: string): number | undefined
  getBoolean(key: string): boolean | undefined
  getBuffer(key: string): ArrayBuffer | undefined
  getJson(key: string): string | undefined
  contains(key: string): boolean
  remove(key: string): boolean
  getAllKeys(): string[]
  clearAll(): void
  flush(): void
  size(): number
  close(): void

  addListener(listener: (key?: string) => void): number
  removeListener(id: number): boolean

  setStringTtl(key: string, value: string, ttlMs: number): void
  setNumberTtl(key: string, value: number, ttlMs: number): void
  setBooleanTtl(key: string, value: boolean, ttlMs: number): void
  setBufferTtl(key: string, value: ArrayBuffer, ttlMs: number): void
  setJsonTtl(key: string, json: string, ttlMs: number): void

  setManyString(keys: string[], values: string[]): void
  getManyString(keys: string[]): (string | null)[]
  applyBatch(packed: ArrayBuffer): void

  setStringAsync(key: string, value: string): Promise<void>
  setNumberAsync(key: string, value: number): Promise<void>
  setBooleanAsync(key: string, value: boolean): Promise<void>
  setBufferAsync(key: string, value: ArrayBuffer): Promise<void>
  setJsonAsync(key: string, json: string): Promise<void>
  getStringAsync(key: string): Promise<string | undefined>
  getNumberAsync(key: string): Promise<number | undefined>
  getBooleanAsync(key: string): Promise<boolean | undefined>
  getBufferAsync(key: string): Promise<ArrayBuffer | undefined>
  getJsonAsync(key: string): Promise<string | undefined>
  containsAsync(key: string): Promise<boolean>
  removeAsync(key: string): Promise<boolean>
  getAllKeysAsync(): Promise<string[]>
  clearAllAsync(): Promise<void>
  flushAsync(): Promise<void>
  setManyStringAsync(keys: string[], values: string[]): Promise<void>
  getManyStringAsync(keys: string[]): Promise<(string | null)[]>
}
