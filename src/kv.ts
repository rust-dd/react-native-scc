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

function getTtlMs(options?: SetOptions): number | undefined {
  const ttl = options?.ttlMs
  if (ttl === undefined) return undefined
  return getPositiveSafeInteger(ttl, 'ttlMs')
}

function serializeJSON(value: unknown): string {
  const json = JSON.stringify(value)
  if (json === undefined) {
    throw new TypeError('value must be JSON-serializable')
  }
  return json
}

function getPositiveSafeInteger(value: number, name: string): number {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new TypeError(`${name} must be a positive safe integer`)
  }
  return value
}

function getOptionalPositiveSafeInteger(
  value: number | undefined,
  name: string
): number | undefined {
  if (value === undefined) return undefined
  return getPositiveSafeInteger(value, name)
}

function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
  return (
    typeof value === 'object' &&
    value !== null &&
    'then' in value &&
    typeof (value as { then?: unknown }).then === 'function'
  )
}

const TAG_STR = 0
const TAG_NUM = 1
const TAG_BOOL = 2
const TAG_BYTES = 3
const TAG_JSON = 4

export const INTERNAL_GET_JSON_TEXT = Symbol('scc.getJSONText')

const textEncoder =
  typeof TextEncoder === 'undefined' ? undefined : new TextEncoder()

/** UTF-8 encode with a manual fallback: JSC and Hermes < RN 0.74 lack TextEncoder. */
function utf8Encode(value: string): Uint8Array {
  if (textEncoder !== undefined) return textEncoder.encode(value)
  const out: number[] = []
  for (let i = 0; i < value.length; i++) {
    const cp = value.codePointAt(i)!
    if (cp > 0xffff) i++
    if (cp < 0x80) out.push(cp)
    else if (cp < 0x800) out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f))
    else if (cp < 0x10000)
      out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f))
    else
      out.push(
        0xf0 | (cp >> 18),
        0x80 | ((cp >> 12) & 0x3f),
        0x80 | ((cp >> 6) & 0x3f),
        0x80 | (cp & 0x3f)
      )
  }
  return new Uint8Array(out)
}

interface EncodedOp {
  del: boolean
  key: string
  tag?: number
  bytes?: Uint8Array
}

function encodeValue(value: KVValue): { tag: number; bytes: Uint8Array } {
  if (typeof value === 'string') {
    return { tag: TAG_STR, bytes: utf8Encode(value) }
  }
  if (typeof value === 'number') {
    const bytes = new Uint8Array(8)
    new DataView(bytes.buffer).setFloat64(0, value, true)
    return { tag: TAG_NUM, bytes }
  }
  if (typeof value === 'boolean') {
    return { tag: TAG_BOOL, bytes: new Uint8Array([value ? 1 : 0]) }
  }
  return { tag: TAG_BYTES, bytes: new Uint8Array(value) }
}

/**
 * Packs a transaction's staged ops into the wire format decoded by the native
 * `applyBatch`: `[u32 count]` then each op `[u8 kind][u32 keyLen][key]` and,
 * for a set (`kind === 1`), `[u8 tag][u32 valLen][val]`. All little-endian.
 */
function encodeBatch(ops: EncodedOp[]): ArrayBuffer {
  const parts = ops.map((op) => ({ ...op, keyBytes: utf8Encode(op.key) }))
  let size = 4
  for (const p of parts) {
    size += 1 + 4 + p.keyBytes.length
    if (!p.del) size += 1 + 4 + p.bytes!.length
  }
  const out = new Uint8Array(size)
  const view = new DataView(out.buffer)
  let off = 0
  view.setUint32(off, parts.length, true)
  off += 4
  for (const p of parts) {
    out[off] = p.del ? 0 : 1
    off += 1
    view.setUint32(off, p.keyBytes.length, true)
    off += 4
    out.set(p.keyBytes, off)
    off += p.keyBytes.length
    if (!p.del) {
      out[off] = p.tag!
      off += 1
      view.setUint32(off, p.bytes!.length, true)
      off += 4
      out.set(p.bytes!, off)
      off += p.bytes!.length
    }
  }
  return out.buffer
}

type PendingWrite =
  | { kind: 'delete' }
  | { kind: 'value'; value: KVValue }
  | { kind: 'json'; json: string }

export interface KVTransaction {
  set(key: string, value: KVValue): void
  setJSON(key: string, value: unknown): void
  getString(key: string): string | undefined
  getNumber(key: string): number | undefined
  getBoolean(key: string): boolean | undefined
  getBuffer(key: string): ArrayBuffer | undefined
  getJSON<T = unknown>(key: string): T | undefined
  contains(key: string): boolean
  delete(key: string): void
}

class TransactionContext implements KVTransaction {
  private readonly writes = new Map<string, PendingWrite>()

  constructor(private readonly store: KV) {}

  set(key: string, value: KVValue): void {
    // Copy staged buffers: kv.set marshals at call time, so later caller-side
    // mutation must not leak into the commit here either.
    this.writes.set(key, {
      kind: 'value',
      value: value instanceof ArrayBuffer ? value.slice(0) : value,
    })
  }

  setJSON(key: string, value: unknown): void {
    this.writes.set(key, { kind: 'json', json: serializeJSON(value) })
  }

  getString(key: string): string | undefined {
    const write = this.writes.get(key)
    if (write !== undefined) {
      return write.kind === 'value' && typeof write.value === 'string'
        ? write.value
        : undefined
    }
    return this.store.getString(key)
  }

  getNumber(key: string): number | undefined {
    const write = this.writes.get(key)
    if (write !== undefined) {
      return write.kind === 'value' && typeof write.value === 'number'
        ? write.value
        : undefined
    }
    return this.store.getNumber(key)
  }

  getBoolean(key: string): boolean | undefined {
    const write = this.writes.get(key)
    if (write !== undefined) {
      return write.kind === 'value' && typeof write.value === 'boolean'
        ? write.value
        : undefined
    }
    return this.store.getBoolean(key)
  }

  getBuffer(key: string): ArrayBuffer | undefined {
    const write = this.writes.get(key)
    if (write !== undefined) {
      return write.kind === 'value' && write.value instanceof ArrayBuffer
        ? write.value.slice(0)
        : undefined
    }
    return this.store.getBuffer(key)
  }

  getJSON<T = unknown>(key: string): T | undefined {
    const write = this.writes.get(key)
    if (write !== undefined) {
      // Parse the staged snapshot so reads match what commit will write and
      // mutating the returned object cannot desync them (KV.getJSON parity).
      return write.kind === 'json' ? (JSON.parse(write.json) as T) : undefined
    }
    return this.store.getJSON<T>(key)
  }

  contains(key: string): boolean {
    const write = this.writes.get(key)
    if (write !== undefined) return write.kind !== 'delete'
    return this.store.contains(key)
  }

  delete(key: string): void {
    this.writes.set(key, { kind: 'delete' })
  }

  drain(): Array<{ key: string; write: PendingWrite }> {
    return [...this.writes].map(([key, write]) => ({ key, write }))
  }
}

let defaultInstance: KV | undefined

export class KV {
  private readonly native: SccKvInstance
  private readonly listeners = new Set<{ listener: KVChangeListener }>()
  private nativeSubscription: number | undefined
  private readonly keyPrefix: string

  private closed = false

  constructor(
    native: SccKvInstance,
    keyPrefix = '',
    private readonly ownsNative = true
  ) {
    this.native = native
    this.keyPrefix = keyPrefix
  }

  /**
   * Fires after every mutation of the underlying store — including writes
   * made through other KV objects opened with the same id. `key` is null
   * after clearAll ("everything changed"). Delivery is asynchronous on the
   * JS thread.
   */
  addOnValueChangedListener(listener: KVChangeListener): KVSubscription {
    if (this.closed) throw new Error('Cannot subscribe to a closed KV instance')
    const entry = { listener }
    this.listeners.add(entry)
    try {
      this.nativeSubscription ??= this.native.addListener((key) => {
        const localKey = this.toLocalChangedKey(key ?? null)
        if (localKey === undefined) return
        for (const current of this.listeners) current.listener(localKey)
      })
    } catch (error) {
      this.listeners.delete(entry)
      throw error
    }
    return {
      remove: () => {
        this.listeners.delete(entry)
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

  /**
   * Stages reads and writes through `tx`, then commits every staged write as one
   * crash-atomic native batch (one WAL record — all of it survives a crash, or
   * none of it). The callback must be synchronous and sees prior staged writes.
   * Concurrent readers can observe individual in-memory key updates while the
   * commit is applied. If the native commit throws (e.g. a background I/O error),
   * no listener fires and the in-process view may be ahead of a restart.
   */
  transaction<T>(callback: (tx: KVTransaction) => T): T {
    const tx = new TransactionContext(this)
    const result = callback(tx)
    if (isPromiseLike(result)) {
      throw new TypeError('transaction callback must be synchronous')
    }
    const staged = tx.drain()
    if (staged.length > 0) {
      const ops = staged.map(({ key, write }): EncodedOp => {
        const fullKey = this.fullKey(key)
        if (write.kind === 'delete') return { del: true, key: fullKey }
        if (write.kind === 'json') {
          return {
            del: false,
            key: fullKey,
            tag: TAG_JSON,
            bytes: utf8Encode(write.json),
          }
        }
        const { tag, bytes } = encodeValue(write.value)
        return { del: false, key: fullKey, tag, bytes }
      })
      this.native.applyBatch(encodeBatch(ops))
    }
    return result
  }

  namespace(prefix: string): KV {
    const normalized = prefix.endsWith(':') ? prefix : `${prefix}:`
    return new KV(this.native, this.fullKey(normalized), false)
  }

  getKeysByPrefix(prefix: string): string[] {
    const fullPrefix = this.fullKey(prefix)
    return this.native.getAllKeys().filter((key) => key.startsWith(fullPrefix))
  }

  /** Deletes the matching snapshot as one native crash-atomic WAL batch. */
  deleteByPrefix(prefix: string): number {
    const keys = this.getKeysByPrefix(prefix)
    if (keys.length === 0) return 0
    this.native.applyBatch(
      encodeBatch(keys.map((key): EncodedOp => ({ del: true, key })))
    )
    return keys.length
  }

  observeJSON<T = unknown, S = T | undefined>(
    key: string,
    selector: (value: T | undefined) => S,
    listener: (selected: S) => void,
    equals: (a: S, b: S) => boolean = Object.is
  ): KVSubscription {
    let selected!: S
    let initialized = false
    const subscription = this.addOnValueChangedListener((changedKey) => {
      if (changedKey !== null && changedKey !== key) return
      const next = selector(this.getJSON<T>(key))
      if (!initialized || !equals(selected, next)) {
        initialized = true
        selected = next
        listener(next)
      }
    })
    try {
      selected = selector(this.getJSON<T>(key))
      initialized = true
      listener(selected)
      return subscription
    } catch (error) {
      subscription.remove()
      throw error
    }
  }

  set(key: string, value: KVValue, options?: SetOptions): void {
    const fullKey =
      this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`
    if (options === undefined) {
      if (typeof value === 'string') this.native.setString(fullKey, value)
      else if (typeof value === 'number') this.native.setNumber(fullKey, value)
      else if (typeof value === 'boolean') this.native.setBoolean(fullKey, value)
      else this.native.setBuffer(fullKey, value)
      return
    }
    const ttl = getTtlMs(options)
    if (ttl !== undefined) {
      if (typeof value === 'string') this.native.setStringTtl(fullKey, value, ttl)
      else if (typeof value === 'number') this.native.setNumberTtl(fullKey, value, ttl)
      else if (typeof value === 'boolean') this.native.setBooleanTtl(fullKey, value, ttl)
      else this.native.setBufferTtl(fullKey, value, ttl)
      return
    }
    if (typeof value === 'string') this.native.setString(fullKey, value)
    else if (typeof value === 'number') this.native.setNumber(fullKey, value)
    else if (typeof value === 'boolean') this.native.setBoolean(fullKey, value)
    else this.native.setBuffer(fullKey, value)
  }

  setJSON(key: string, value: unknown, options?: SetOptions): void {
    const ttl = getTtlMs(options)
    const json = serializeJSON(value)
    const fullKey = this.fullKey(key)
    if (ttl !== undefined) {
      this.native.setJsonTtl(fullKey, json, ttl)
      return
    }
    this.native.setJson(fullKey, json)
  }

  getString(key: string): string | undefined {
    return this.native.getString(
      this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`
    )
  }

  getNumber(key: string): number | undefined {
    return this.native.getNumber(
      this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`
    )
  }

  getBoolean(key: string): boolean | undefined {
    return this.native.getBoolean(
      this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`
    )
  }

  getBuffer(key: string): ArrayBuffer | undefined {
    return this.native.getBuffer(
      this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`
    )
  }

  getJSON<T = unknown>(key: string): T | undefined {
    const json = this[INTERNAL_GET_JSON_TEXT](key)
    return json === undefined ? undefined : (JSON.parse(json) as T)
  }

  [INTERNAL_GET_JSON_TEXT](key: string): string | undefined {
    return this.native.getJson(
      this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`
    )
  }

  contains(key: string): boolean {
    return this.native.contains(
      this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`
    )
  }

  delete(key: string): boolean {
    return this.native.remove(
      this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`
    )
  }

  getAllKeys(): string[] {
    if (this.keyPrefix === '') return this.native.getAllKeys()
    return this.getKeysByPrefix('').map((key) => key.slice(this.keyPrefix.length))
  }

  clearAll(): number {
    if (this.keyPrefix !== '') return this.deleteByPrefix('')
    const removed = this.size
    this.native.clearAll()
    return removed
  }

  flush(): void {
    this.native.flush()
  }

  /** Batch string write — one bridge crossing for the whole record set. */
  setMany(entries: Record<string, string>): void {
    const keys = Object.keys(entries)
    if (keys.length === 0) return
    this.native.setManyString(this.fullKeys(keys), Object.values(entries))
  }

  /** Batch string read; missing keys come back as undefined. */
  getMany(keys: string[]): (string | undefined)[] {
    if (keys.length === 0) return []
    const values: (string | null | undefined)[] =
      this.native.getManyString(this.fullKeys(keys))
    for (let index = 0; index < values.length; index++) {
      if (values[index] === null) values[index] = undefined
    }
    return values as (string | undefined)[]
  }

  get size(): number {
    if (this.keyPrefix !== '') return this.getAllKeys().length
    return this.native.size()
  }

  /**
   * Closes this root store and releases its native change listener. Namespace
   * views are non-owning and must be discarded instead of closed.
   */
  close(): void {
    if (!this.ownsNative) {
      throw new Error('Cannot close a KV namespace; close its root KV instance')
    }
    if (this.closed) return
    if (this.nativeSubscription !== undefined) {
      this.native.removeListener(this.nativeSubscription)
      this.nativeSubscription = undefined
    }
    this.listeners.clear()
    this.native.close()
    this.closed = true
    if (defaultInstance === this) defaultInstance = undefined
  }

  setAsync(key: string, value: KVValue): Promise<void> {
    const fullKey = this.fullKey(key)
    if (typeof value === 'string') return this.native.setStringAsync(fullKey, value)
    if (typeof value === 'number') return this.native.setNumberAsync(fullKey, value)
    if (typeof value === 'boolean')
      return this.native.setBooleanAsync(fullKey, value)
    return this.native.setBufferAsync(fullKey, value)
  }

  async setJSONAsync(key: string, value: unknown): Promise<void> {
    const json = serializeJSON(value)
    return this.native.setJsonAsync(this.fullKey(key), json)
  }

  getStringAsync(key: string): Promise<string | undefined> {
    return this.native.getStringAsync(this.fullKey(key))
  }

  getNumberAsync(key: string): Promise<number | undefined> {
    return this.native.getNumberAsync(this.fullKey(key))
  }

  getBooleanAsync(key: string): Promise<boolean | undefined> {
    return this.native.getBooleanAsync(this.fullKey(key))
  }

  getBufferAsync(key: string): Promise<ArrayBuffer | undefined> {
    return this.native.getBufferAsync(this.fullKey(key))
  }

  async getJSONAsync<T = unknown>(key: string): Promise<T | undefined> {
    const json = await this.native.getJsonAsync(this.fullKey(key))
    return json === undefined ? undefined : (JSON.parse(json) as T)
  }

  containsAsync(key: string): Promise<boolean> {
    return this.native.containsAsync(this.fullKey(key))
  }

  deleteAsync(key: string): Promise<boolean> {
    return this.native.removeAsync(this.fullKey(key))
  }

  async getAllKeysAsync(): Promise<string[]> {
    if (this.keyPrefix === '') return this.native.getAllKeysAsync()
    const keys = await this.getFullKeysByPrefixAsync('')
    return keys.map((key) => key.slice(this.keyPrefix.length))
  }

  async clearAllAsync(): Promise<void> {
    if (this.keyPrefix !== '') {
      const keys = await this.getFullKeysByPrefixAsync('')
      await Promise.all(keys.map((key) => this.native.removeAsync(key)))
      return
    }
    return this.native.clearAllAsync()
  }

  flushAsync(): Promise<void> {
    return this.native.flushAsync()
  }

  setManyAsync(entries: Record<string, string>): Promise<void> {
    const keys = Object.keys(entries)
    if (keys.length === 0) return Promise.resolve()
    return this.native.setManyStringAsync(
      this.fullKeys(keys),
      Object.values(entries)
    )
  }

  async getManyAsync(keys: string[]): Promise<(string | undefined)[]> {
    if (keys.length === 0) return []
    const values: (string | null | undefined)[] =
      await this.native.getManyStringAsync(this.fullKeys(keys))
    for (let index = 0; index < values.length; index++) {
      if (values[index] === null) values[index] = undefined
    }
    return values as (string | undefined)[]
  }

  private fullKey(key: string): string {
    return this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`
  }

  private fullKeys(keys: string[]): string[] {
    if (this.keyPrefix === '') return keys
    return keys.map((key) => `${this.keyPrefix}${key}`)
  }

  private async getFullKeysByPrefixAsync(prefix: string): Promise<string[]> {
    const fullPrefix = this.fullKey(prefix)
    const keys = await this.native.getAllKeysAsync()
    return keys.filter((key) => key.startsWith(fullPrefix))
  }

  private toLocalChangedKey(key: string | null): string | null | undefined {
    if (key === null) return null
    if (this.keyPrefix === '') return key
    if (!key.startsWith(this.keyPrefix)) return undefined
    return key.slice(this.keyPrefix.length)
  }
}

export function createKV(options: KVOptions = {}): KV {
  const id = options.id ?? 'default'
  const maxEntries = getOptionalPositiveSafeInteger(
    options.maxEntries,
    'maxEntries'
  )
  const ttlSweepIntervalMs = getOptionalPositiveSafeInteger(
    options.ttlSweepIntervalMs,
    'ttlSweepIntervalMs'
  )
  if (options.persistence === 'none') {
    return new KV(getFactory().inMemory(id, maxEntries, ttlSweepIntervalMs))
  }
  const dir = options.path ?? getBaseDirectory()
  const strict = options.durability === 'strict'
  return new KV(
    getFactory().open(
      dir,
      id,
      strict,
      options.recreate ?? false,
      options.encryptionKey,
      maxEntries,
      ttlSweepIntervalMs
    )
  )
}

export function getDefaultKV(): KV {
  defaultInstance ??= createKV()
  return defaultInstance
}
