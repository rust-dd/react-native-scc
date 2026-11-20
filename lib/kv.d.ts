import type { SccKvInstance } from './specs/SccKvInstance.nitro';
import type { KVOptions, KVValue, SetOptions } from './types';
export type KVChangeListener = (key: string | null) => void;
export interface KVSubscription {
    remove(): void;
}
export declare const INTERNAL_GET_JSON_TEXT: unique symbol;
export declare const INTERNAL_IS_CLOSED: unique symbol;
export declare const INTERNAL_SET_JSON: unique symbol;
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
interface NativeCalls {
    getString(key: string): string | undefined;
    getNumber(key: string): number | undefined;
    getBoolean(key: string): boolean | undefined;
    getJson(key: string): string | undefined;
    contains(key: string): boolean;
    remove(key: string): boolean;
    setString(key: string, value: string): void;
    setNumber(key: string, value: number): void;
    setBoolean(key: string, value: boolean): void;
    getManyString(keys: string[]): (string | null | undefined)[];
    setManyString(keys: string[], values: string[]): void;
}
/** State a root store shares with its namespace views: one native listener, one fast path. */
declare class StoreHub {
    readonly native: SccKvInstance;
    readonly calls: NativeCalls;
    readonly members: Set<KV>;
    readonly views: Map<string, KV>;
    subscription: number | undefined;
    deliveries: number;
    closed: boolean;
    constructor(native: SccKvInstance, calls: NativeCalls);
}
export declare class KV {
    private readonly ownsNative;
    private readonly storeKey?;
    private readonly native;
    private readonly calls;
    private readonly listeners;
    private readonly keyListeners;
    private readonly hub;
    private readonly keyPrefix;
    constructor(native: SccKvInstance, keyPrefix?: string, ownsNative?: boolean, storeKey?: string | undefined, hub?: StoreHub);
    /** Fires asynchronously on the JS thread after every mutation from any handle; `key` is null after clearAll. */
    addOnValueChangedListener(listener: KVChangeListener): KVSubscription;
    /** Like addOnValueChangedListener, but only for `key` (and clearAll, as null). */
    addOnKeyChangedListener(key: string, listener: KVChangeListener): KVSubscription;
    private subscribe;
    private keyEntries;
    private unsubscribe;
    private static deliver;
    private dispatch;
    /** Commits staged writes as one crash-atomic WAL record (readers may see keys land one by one); the callback must be sync.
     *  If the commit throws, no listener fires and memory may be ahead of what survives a restart. */
    transaction<T>(callback: (tx: KVTransaction) => T): T;
    /** Views are cached per prefix, so a namespace created during render keeps its identity. */
    namespace(prefix: string): KV;
    getKeysByPrefix(prefix: string): string[];
    /** Deletes the matching snapshot as one native crash-atomic WAL batch. */
    deleteByPrefix(prefix: string): number;
    observeJSON<T = unknown, S = T | undefined>(key: string, selector: (value: T | undefined) => S, listener: (selected: S) => void, equals?: (a: S, b: S) => boolean): KVSubscription;
    set(key: string, value: KVValue, options?: SetOptions): void;
    setJSON(key: string, value: unknown, options?: SetOptions): void;
    [INTERNAL_SET_JSON](key: string, value: unknown): string;
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
    /** Closes the root store and its namespace views; a view itself cannot be closed. */
    close(): void;
    get [INTERNAL_IS_CLOSED](): boolean;
    private detach;
    private abandon;
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
    private toLocalKeys;
    private toLocalChangedKey;
}
export declare function createKV(options?: KVOptions): KV;
/** Sets the options hooks and adapters without an explicit `kv` open their store with. */
export declare function configureDefaultKV(options: KVOptions): void;
export declare function getDefaultKV(): KV;
export {};
