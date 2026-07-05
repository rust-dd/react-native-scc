import { NitroModules } from 'react-native-nitro-modules'
import type { SccKv } from './specs/SccKv.nitro'
import type { SccKvInstance } from './specs/SccKvInstance.nitro'
import type { SccKvPlatformContext } from './specs/SccKvPlatformContext.nitro'
import type { KVOptions, KVValue, SetOptions } from './types'

let factory: SccKv | undefined
let platformContext: SccKvPlatformContext | undefined

function getFactory(): SccKv {
  factory ??= NitroModules.createHybridObject<SccKv>('SccKv')
  return factory
}

function getBaseDirectory(): string {
  platformContext ??=
    NitroModules.createHybridObject<SccKvPlatformContext>('SccKvPlatformContext')
  return platformContext.getBaseDirectory()
}

export type KVChangeListener = (key: string | null) => void

export interface KVSubscription {
  remove(): void
}

export class KV {
  private readonly native: SccKvInstance
  private readonly listeners = new Set<KVChangeListener>()
  private nativeSubscription: number | undefined

  constructor(native: SccKvInstance) {
    this.native = native
  }

  /**
   * Fires after every mutation of the underlying store — including writes
   * made through other KV objects opened with the same id. `key` is null
   * after clearAll ("everything changed"). Delivery is asynchronous on the
   * JS thread.
   */
  addOnValueChangedListener(listener: KVChangeListener): KVSubscription {
    this.listeners.add(listener)
    this.nativeSubscription ??= this.native.addListener((key) => {
      for (const l of this.listeners) l(key ?? null)
    })
    return {
      remove: () => {
        this.listeners.delete(listener)
        if (
          this.listeners.size === 0 &&
          this.nativeSubscription !== undefined
        ) {
          this.native.removeListener(this.nativeSubscription)
          this.nativeSubscription = undefined
        }
      },
    }
  }

  set(key: string, value: KVValue, options?: SetOptions): void {
    const ttl = options?.ttlMs
    if (ttl !== undefined) {
      if (typeof value === 'string') this.native.setStringTtl(key, value, ttl)
      else if (typeof value === 'number') this.native.setNumberTtl(key, value, ttl)
      else if (typeof value === 'boolean') this.native.setBooleanTtl(key, value, ttl)
      else this.native.setBufferTtl(key, value, ttl)
      return
    }
    if (typeof value === 'string') this.native.setString(key, value)
    else if (typeof value === 'number') this.native.setNumber(key, value)
    else if (typeof value === 'boolean') this.native.setBoolean(key, value)
    else this.native.setBuffer(key, value)
  }

  setJSON(key: string, value: unknown, options?: SetOptions): void {
    const ttl = options?.ttlMs
    if (ttl !== undefined) {
      this.native.setJsonTtl(key, JSON.stringify(value), ttl)
      return
    }
    this.native.setJson(key, JSON.stringify(value))
  }

  getString(key: string): string | undefined {
    return this.native.getString(key)
  }

  getNumber(key: string): number | undefined {
    return this.native.getNumber(key)
  }

  getBoolean(key: string): boolean | undefined {
    return this.native.getBoolean(key)
  }

  getBuffer(key: string): ArrayBuffer | undefined {
    return this.native.getBuffer(key)
  }

  getJSON<T = unknown>(key: string): T | undefined {
    const json = this.native.getJson(key)
    return json === undefined ? undefined : (JSON.parse(json) as T)
  }

  contains(key: string): boolean {
    return this.native.contains(key)
  }

  delete(key: string): boolean {
    return this.native.remove(key)
  }

  getAllKeys(): string[] {
    return this.native.getAllKeys()
  }

  clearAll(): void {
    this.native.clearAll()
  }

  flush(): void {
    this.native.flush()
  }

  /** Batch string write — one bridge crossing for the whole record set. */
  setMany(entries: Record<string, string>): void {
    const keys = Object.keys(entries)
    this.native.setManyString(
      keys,
      keys.map((k) => entries[k]!)
    )
  }

  /** Batch string read; missing keys come back as undefined. */
  getMany(keys: string[]): (string | undefined)[] {
    return this.native.getManyString(keys).map((v) => v ?? undefined)
  }

  get size(): number {
    return this.native.size()
  }

  close(): void {
    this.native.close()
  }

  setAsync(key: string, value: KVValue): Promise<void> {
    if (typeof value === 'string') return this.native.setStringAsync(key, value)
    if (typeof value === 'number') return this.native.setNumberAsync(key, value)
    if (typeof value === 'boolean')
      return this.native.setBooleanAsync(key, value)
    return this.native.setBufferAsync(key, value)
  }

  setJSONAsync(key: string, value: unknown): Promise<void> {
    return this.native.setJsonAsync(key, JSON.stringify(value))
  }

  getStringAsync(key: string): Promise<string | undefined> {
    return this.native.getStringAsync(key)
  }

  getNumberAsync(key: string): Promise<number | undefined> {
    return this.native.getNumberAsync(key)
  }

  getBooleanAsync(key: string): Promise<boolean | undefined> {
    return this.native.getBooleanAsync(key)
  }

  getBufferAsync(key: string): Promise<ArrayBuffer | undefined> {
    return this.native.getBufferAsync(key)
  }

  async getJSONAsync<T = unknown>(key: string): Promise<T | undefined> {
    const json = await this.native.getJsonAsync(key)
    return json === undefined ? undefined : (JSON.parse(json) as T)
  }

  containsAsync(key: string): Promise<boolean> {
    return this.native.containsAsync(key)
  }

  deleteAsync(key: string): Promise<boolean> {
    return this.native.removeAsync(key)
  }

  getAllKeysAsync(): Promise<string[]> {
    return this.native.getAllKeysAsync()
  }

  clearAllAsync(): Promise<void> {
    return this.native.clearAllAsync()
  }

  flushAsync(): Promise<void> {
    return this.native.flushAsync()
  }

  setManyAsync(entries: Record<string, string>): Promise<void> {
    const keys = Object.keys(entries)
    return this.native.setManyStringAsync(
      keys,
      keys.map((k) => entries[k]!)
    )
  }

  async getManyAsync(keys: string[]): Promise<(string | undefined)[]> {
    const values = await this.native.getManyStringAsync(keys)
    return values.map((v) => v ?? undefined)
  }
}

export function createKV(options: KVOptions = {}): KV {
  const id = options.id ?? 'default'
  if (options.persistence === 'none') {
    return new KV(getFactory().inMemory(id))
  }
  const dir = options.path ?? getBaseDirectory()
  const strict = options.durability === 'strict'
  return new KV(
    getFactory().open(dir, id, strict, options.recreate ?? false, options.encryptionKey)
  )
}

let defaultInstance: KV | undefined

export function getDefaultKV(): KV {
  defaultInstance ??= createKV()
  return defaultInstance
}
