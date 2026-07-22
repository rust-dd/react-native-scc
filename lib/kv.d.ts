import type { SccKvInstance } from './specs/SccKvInstance.nitro';
import type { KVOptions, KVValue, SetOptions } from './types';
export type KVChangeListener = (key: string | null) => void;
export interface KVSubscription {
    remove(): void;
}
export declare const INTERNAL_GET_JSON_TEXT: unique symbol;
export interface KVTransaction {
    set(key: string, value: KVValue): void;
    setJSON(key: string, value: unknown): void;
    getString(key: string): string | undefined;
    getNumber(key: string): number | undefined;
    getBoolean(key: string): boolean | undefined;
    getBuffer(key: string): ArrayBuffer | undefined;
    getJSON<T = unknown>(key: string): T | undefined;
    contains(key: string): boolean;
    delete(key: string): void;
}
export declare class KV {
    private readonly ownsNative;
    private readonly native;
    private readonly listeners;
    private nativeSubscription;
    private readonly keyPrefix;
    private closed;
    constructor(native: SccKvInstance, keyPrefix?: string, ownsNative?: boolean);
    /**
     * Fires after every mutation of the underlying store — including writes
     * made through other KV objects opened with the same id. `key` is null
     * after clearAll ("everything changed"). Delivery is asynchronous on the
     * JS thread.
     */
    addOnValueChangedListener(listener: KVChangeListener): KVSubscription;
    /**
     * Stages reads and writes through `tx`, then commits every staged write as one
     * crash-atomic native batch (one WAL record — all of it survives a crash, or
     * none of it). The callback must be synchronous and sees prior staged writes.
     * Concurrent readers can observe individual in-memory key updates while the
     * commit is applied. If the native commit throws (e.g. a background I/O error),
     * no listener fires and the in-process view may be ahead of a restart.
     */
    transaction<T>(callback: (tx: KVTransaction) => T): T;
    namespace(prefix: string): KV;
    getKeysByPrefix(prefix: string): string[];
    /** Deletes the matching snapshot as one native crash-atomic WAL batch. */
    deleteByPrefix(prefix: string): number;
    observeJSON<T = unknown, S = T | undefined>(key: string, selector: (value: T | undefined) => S, listener: (selected: S) => void, equals?: (a: S, b: S) => boolean): KVSubscription;
    set(key: string, value: KVValue, options?: SetOptions): void;
    setJSON(key: string, value: unknown, options?: SetOptions): void;
    getString(key: string): string | undefined;
    getNumber(key: string): number | undefined;
    getBoolean(key: string): boolean | undefined;
    getBuffer(key: string): ArrayBuffer | undefined;
    getJSON<T = unknown>(key: string): T | undefined;
    [INTERNAL_GET_JSON_TEXT](key: string): string | undefined;
    contains(key: string): boolean;
    delete(key: string): boolean;
    getAllKeys(): string[];
    clearAll(): number;
    flush(): void;
    /** Batch string write — one bridge crossing for the whole record set. */
    setMany(entries: Record<string, string>): void;
    /** Batch string read; missing keys come back as undefined. */
    getMany(keys: string[]): (string | undefined)[];
    get size(): number;
    /**
     * Closes this root store and releases its native change listener. Namespace
     * views are non-owning and must be discarded instead of closed.
     */
    close(): void;
    setAsync(key: string, value: KVValue): Promise<void>;
    setJSONAsync(key: string, value: unknown): Promise<void>;
    getStringAsync(key: string): Promise<string | undefined>;
    getNumberAsync(key: string): Promise<number | undefined>;
    getBooleanAsync(key: string): Promise<boolean | undefined>;
    getBufferAsync(key: string): Promise<ArrayBuffer | undefined>;
    getJSONAsync<T = unknown>(key: string): Promise<T | undefined>;
    containsAsync(key: string): Promise<boolean>;
    deleteAsync(key: string): Promise<boolean>;
    getAllKeysAsync(): Promise<string[]>;
    clearAllAsync(): Promise<void>;
    flushAsync(): Promise<void>;
    setManyAsync(entries: Record<string, string>): Promise<void>;
    getManyAsync(keys: string[]): Promise<(string | undefined)[]>;
    private fullKey;
    private fullKeys;
    private getFullKeysByPrefixAsync;
    private toLocalChangedKey;
}
export declare function createKV(options?: KVOptions): KV;
export declare function getDefaultKV(): KV;
