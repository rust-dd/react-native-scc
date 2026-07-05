import type { SccKvInstance } from './specs/SccKvInstance.nitro';
import type { KVOptions, KVValue, SetOptions } from './types';
export type KVChangeListener = (key: string | null) => void;
export interface KVSubscription {
    remove(): void;
}
export declare class KV {
    private readonly native;
    private readonly listeners;
    private nativeSubscription;
    constructor(native: SccKvInstance);
    /**
     * Fires after every mutation of the underlying store — including writes
     * made through other KV objects opened with the same id. `key` is null
     * after clearAll ("everything changed"). Delivery is asynchronous on the
     * JS thread.
     */
    addOnValueChangedListener(listener: KVChangeListener): KVSubscription;
    set(key: string, value: KVValue, options?: SetOptions): void;
    setJSON(key: string, value: unknown, options?: SetOptions): void;
    getString(key: string): string | undefined;
    getNumber(key: string): number | undefined;
    getBoolean(key: string): boolean | undefined;
    getBuffer(key: string): ArrayBuffer | undefined;
    getJSON<T = unknown>(key: string): T | undefined;
    contains(key: string): boolean;
    delete(key: string): boolean;
    getAllKeys(): string[];
    clearAll(): void;
    flush(): void;
    /** Batch string write — one bridge crossing for the whole record set. */
    setMany(entries: Record<string, string>): void;
    /** Batch string read; missing keys come back as undefined. */
    getMany(keys: string[]): (string | undefined)[];
    get size(): number;
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
}
export declare function createKV(options?: KVOptions): KV;
export declare function getDefaultKV(): KV;
