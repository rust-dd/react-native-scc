import { KV } from './kv';
/** Reactive string value; re-renders when the key changes. */
export declare function useKVString(key: string, kv?: KV): [string | undefined, (value: string | undefined) => void];
/** Reactive number value; re-renders when the key changes. */
export declare function useKVNumber(key: string, kv?: KV): [number | undefined, (value: number | undefined) => void];
/** Reactive boolean value; re-renders when the key changes. */
export declare function useKVBoolean(key: string, kv?: KV): [boolean | undefined, (value: boolean | undefined) => void];
/** Reactive ArrayBuffer value; re-renders when the key changes. */
export declare function useKVBuffer(key: string, kv?: KV): [ArrayBuffer | undefined, (value: ArrayBuffer | undefined) => void];
/** Reactive JSON value; re-renders when the key changes. */
export declare function useKVJSON<T = unknown>(key: string, kv?: KV): [T | undefined, (value: T | undefined) => void];
