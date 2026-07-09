import { KV } from './kv';
export declare function useKVString(key: string, kv?: KV): [string | undefined, (value: string | undefined) => void];
export declare function useKVNumber(key: string, kv?: KV): [number | undefined, (value: number | undefined) => void];
export declare function useKVBoolean(key: string, kv?: KV): [boolean | undefined, (value: boolean | undefined) => void];
export declare function useKVBuffer(key: string, kv?: KV): [ArrayBuffer | undefined, (value: ArrayBuffer | undefined) => void];
export declare function useKVJSON<T = unknown>(key: string, kv?: KV): [T | undefined, (value: T | undefined) => void];
